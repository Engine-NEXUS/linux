// Hand mode camera window: camera -> MediaPipe Hand Landmarker -> 21 landmarks -> Rust (`hand_frame`).
// Rust decides what the hand means and sends HUD state back (`hand:state`). No frame ever leaves this
// window: only numbers cross IPC. The window exists only while hand mode is on.
//
// Speed (field tests 1-2): the model is fast (~27 ms); the camera pipeline was delivering ~6 frames/s.
//  * Frames come straight from the camera track (MediaStreamTrackProcessor) when available, not from
//    requestVideoFrameCallback (which follows page presentation). BOTH rates are counted and reported
//    every 5 s (`hand_stats`) so the console says which path is slow.
//  * The picture shown is the frame that was analysed (skeleton always sits on the hand).
//  * Capture time (not processing start) is reported, so latency numbers include the camera.
//  * Camera and model load in parallel; GPU vs CPU delegate is timed on the first frames.
import { FilesetResolver, HandLandmarker } from "@mediapipe/tasks-vision";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./hand.css";
import {
  HAND_CONNECTIONS,
  cameraHint,
  chooseDelegate,
  isLikelyCovered,
  median,
  needsCpuTrial,
  percentile,
  shouldProcessFrame,
  statusLine,
  toPayload,
  type HudState,
} from "./payload";

const video = document.getElementById("cam") as HTMLVideoElement;
const canvas = document.getElementById("skeleton") as HTMLCanvasElement;
const ctx = canvas.getContext("2d")!;
const statusEl = document.getElementById("status")!;
const badge = document.getElementById("badge")!;
const flash = document.getElementById("flash")!;
const fpsNote = document.getElementById("fpsnote")!;
const dots = Array.from(document.querySelectorAll("#dots i"));
const map = document.getElementById("map") as HTMLCanvasElement;
const mctx = map.getContext("2d")!;

const WARMUP_FRAMES = 4; // first frames include shader/wasm warm-up: not representative
const TRIAL_FRAMES = 25;

type Delegate = "GPU" | "CPU";
type Source = "track" | "rvfc";
type LatencyKind = "capture" | "track-lower-bound" | "arrival";

let inflight = false;
let frameIdx = 0;
let lastHandMs = Date.now();
let lastSeq = 0;
let flashTimer: number | undefined;
// Sustained GPU contention, as last reported back by Rust on `hand:state` (Rust computes it from
// real infer-time data already flowing through `hand_frame`; this window just reacts to it by
// easing off its own frame rate — see `shouldProcessFrame`).
let gpuBusy = false;
// Camera-cover debounce: wall-clock time since frames first looked covered, or null while they
// don't. ~2.5 s of continuous low-signal frames before concluding "covered"; any normal frame
// clears it immediately.
let coveredSinceMs: number | null = null;
let covered = false;
const COVERED_DEBOUNCE_MS = 2500;
// A tiny offscreen canvas, reused every frame, for the brightness/variance sample — far cheaper
// than the MediaPipe inference already running on the same frame.
const covCanvas = document.createElement("canvas");
covCanvas.width = 16;
covCanvas.height = 16;
const covCtx = covCanvas.getContext("2d", { willReadFrequently: true })!;

/** Downsample to 16x16 and return [average brightness 0-255, variance] — enough to tell a
 * covered lens (dark AND flat) apart from a dim room (dark but still textured). */
function sampleCoverage(src: CanvasImageSource, w: number, h: number): [number, number] {
  covCtx.drawImage(src, 0, 0, w, h, 0, 0, 16, 16);
  const data = covCtx.getImageData(0, 0, 16, 16).data;
  let sum = 0;
  const n = 256;
  const gray = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    const o = i * 4;
    const g = (data[o] + data[o + 1] + data[o + 2]) / 3;
    gray[i] = g;
    sum += g;
  }
  const avg = sum / n;
  let variance = 0;
  for (let i = 0; i < n; i++) variance += (gray[i] - avg) ** 2;
  variance /= n;
  return [avg, variance];
}

async function fail(message: string): Promise<never> {
  statusEl.textContent = message;
  await invoke("hand_error", { message }).catch(() => {});
  throw new Error(message);
}

function draw(src: CanvasImageSource, w: number, h: number, hand: { x: number; y: number }[] | undefined) {
  // Resizing a canvas reallocates its bitmap: do it only when the camera size actually changes.
  if (canvas.width !== w) canvas.width = w;
  if (canvas.height !== h) canvas.height = h;
  // The processed frame itself, so the skeleton can never trail a live preview.
  ctx.drawImage(src, 0, 0, w, h);
  if (!hand) return;
  ctx.lineWidth = 3;
  ctx.strokeStyle = "rgba(80,255,160,.9)";
  ctx.beginPath();
  for (const [a, b] of HAND_CONNECTIONS) {
    ctx.moveTo(hand[a].x * w, hand[a].y * h);
    ctx.lineTo(hand[b].x * w, hand[b].y * h);
  }
  ctx.stroke();
  ctx.fillStyle = "#ff5a5a";
  for (const p of hand) ctx.fillRect(p.x * w - 2, p.y * h - 2, 4, 4);
}

function renderState(s: HudState) {
  gpuBusy = s.gpu_busy;
  statusEl.textContent = statusLine(s);
  badge.textContent = s.armed ? (s.live ? "LIVE" : "ARMED · PREVIEW") : "IDLE";
  badge.className = "badge" + (s.armed ? (s.live ? " live" : " armed") : "");
  dots.forEach((d, i) => d.classList.toggle("on", i < s.arm_stage));
  if (s.last_action && s.action_seq !== lastSeq) {
    lastSeq = s.action_seq;
    flash.textContent = s.last_action.replace("_", " ");
    flash.classList.add("on");
    window.clearTimeout(flashTimer);
    flashTimer = window.setTimeout(() => flash.classList.remove("on"), 450);
  }
  // Mini screen map: where Rust PREDICTS the cursor is (and where it would click in preview).
  mctx.clearRect(0, 0, map.width, map.height);
  const [sw, sh] = s.screen;
  if (sw > 0 && sh > 0) {
    mctx.fillStyle = s.pressed ? "#f0b429" : s.armed ? "#4ade80" : "#6b7280";
    mctx.beginPath();
    mctx.arc((s.cursor[0] / sw) * map.width, (s.cursor[1] / sh) * map.height, s.pressed ? 4 : 3, 0, Math.PI * 2);
    mctx.fill();
  }
}

// ── camera ────────────────────────────────────────────────────────────────────────────────────

async function openCamera() {
  const stream = await navigator.mediaDevices.getUserMedia({
    video: { width: { ideal: 640 }, height: { ideal: 480 }, frameRate: { ideal: 30 } },
    audio: false,
  });
  const track = stream.getVideoTracks()[0];
  // Dim rooms make many webcams cut their frame rate; ask for a floor and continuous exposure.
  let asked = "";
  try {
    await track.applyConstraints({ frameRate: { min: 24, ideal: 30 } });
    asked = "fps>=24 ok";
  } catch {
    asked = "fps>=24 refused";
  }
  const s = track.getSettings() as MediaTrackSettings & { exposureMode?: string; exposureTime?: number };
  return {
    stream,
    track,
    desc: `${s.width ?? "?"}x${s.height ?? "?"}@${Math.round(s.frameRate ?? 0)} (${asked})`,
    driverFps: s.frameRate ?? 0,
    exposure: `${s.exposureMode ?? "?"}${s.exposureTime ? "/" + s.exposureTime : ""}`,
  };
}

async function loadModel() {
  const t0 = performance.now();
  const fileset = await FilesetResolver.forVisionTasks("/hand/wasm");
  const make = (delegate: Delegate) =>
    HandLandmarker.createFromOptions(fileset, {
      baseOptions: { modelAssetPath: "/hand/hand_landmarker.task", delegate },
      runningMode: "VIDEO",
      numHands: 1,
      minHandDetectionConfidence: 0.6,
      minHandPresenceConfidence: 0.5,
      minTrackingConfidence: 0.5,
    });
  let delegate: Delegate = "GPU";
  let landmarker: HandLandmarker;
  try {
    landmarker = await make("GPU");
  } catch {
    delegate = "CPU";
    landmarker = await make("CPU");
  }
  return { make, landmarker, delegate, loadMs: performance.now() - t0 };
}

async function batteryText(): Promise<string> {
  try {
    const b = await (navigator as unknown as { getBattery?: () => Promise<{ charging: boolean; level: number }> }).getBattery?.();
    return b ? `${b.charging ? "charging" : "BATTERY"} ${Math.round(b.level * 100)}%` : "unknown";
  } catch {
    return "unknown";
  }
}

async function boot() {
  statusEl.textContent = "starting camera and model…";
  // Camera and model load at the same time (they used to run one after the other).
  const [camR, modelR, cfgR] = await Promise.allSettled([
    openCamera(),
    loadModel(),
    invoke<{ frame_source: string; show_camera: boolean }>("hand_config"),
  ]);
  if (camR.status === "rejected") return fail(`camera unavailable (${(camR.reason as Error)?.name ?? "error"})`);
  if (modelR.status === "rejected") return fail(`hand model failed to load (${(modelR.reason as Error)?.message})`);
  const cam = camR.value;
  const model = modelR.value;
  const cfg = cfgR.status === "fulfilled" ? cfgR.value.frame_source : "auto";
  // Default: nothing is drawn or decoded here (the window is an invisible 8x8 page). `handShowCamera`
  // turns the picture and skeleton back on for debugging.
  const preview = cfgR.status === "fulfilled" ? cfgR.value.show_camera : false;
  if (!preview) document.getElementById("hand-root")!.style.display = "none";

  const MSTP = (window as unknown as {
    MediaStreamTrackProcessor?: new (init: { track: MediaStreamTrack; maxBufferSize?: number }) => { readable: ReadableStream<VideoFrame> };
  }).MediaStreamTrackProcessor;
  const source: Source = cfg === "rvfc" || !MSTP ? "rvfc" : "track";

  // The picture element is only needed to show it, or as the fallback frame source.
  const needVideo = preview || !MSTP || source === "rvfc";
  if (needVideo) {
    video.srcObject = cam.stream;
    await video.play().catch(() => {});
  }

  let landmarker = model.landmarker;
  let delegate = model.delegate;
  statusEl.textContent = "show your palm";

  // ── delegate calibration (first frames) ───────────────────────────────────────────────────
  let phase: "gpu" | "cpu" | "done" = delegate === "GPU" ? "gpu" : "done";
  let samples: number[] = [];
  let gpuMs: number[] = [];
  const gpuLm: HandLandmarker | null = delegate === "GPU" ? landmarker : null;
  let building = false;
  let seen = 0;

  const info = (calibration: string, calibratedMs = 0) =>
    void invoke("hand_info", {
      info: { delegate, camera: `${cam.desc} source=${source}`, load_ms: model.loadMs, calibration, calibrated_ms: calibratedMs },
    }).catch(() => {});

  const finish = (pick: Delegate, note: string, calibratedMs: number) => {
    if (pick === "GPU" && gpuLm) {
      if (landmarker !== gpuLm) landmarker.close();
      landmarker = gpuLm;
    } else if (pick === "CPU" && gpuLm && landmarker !== gpuLm) {
      gpuLm.close();
    }
    delegate = pick;
    phase = "done";
    info(note, calibratedMs);
  };
  // No numeric trial ran (GPU was never available to time), so Rust has no baseline to compare
  // against later — contention detection simply stays off for this session, which is safe.
  if (phase === "done") info("GPU unavailable, CPU only");

  const calibrate = (inferMs: number) => {
    if (phase === "done" || building) return;
    if (++seen <= WARMUP_FRAMES) return;
    samples.push(inferMs);
    if (samples.length < TRIAL_FRAMES) return;
    if (phase === "gpu") {
      gpuMs = samples;
      samples = [];
      seen = 0;
      if (!needsCpuTrial(gpuMs)) {
        finish("GPU", `GPU p50 ${median(gpuMs).toFixed(1)} ms (fast enough, CPU not tried)`, median(gpuMs));
        return;
      }
      building = true;
      model
        .make("CPU")
        .then((lm) => {
          landmarker = lm;
          phase = "cpu";
        })
        .catch(() => finish("GPU", `GPU p50 ${median(gpuMs).toFixed(1)} ms (CPU unavailable)`, median(gpuMs)))
        .finally(() => {
          building = false;
        });
    } else {
      const pick = chooseDelegate(gpuMs, samples);
      const chosenMs = pick === "GPU" ? median(gpuMs) : median(samples);
      finish(pick, `GPU p50 ${median(gpuMs).toFixed(1)} ms vs CPU p50 ${median(samples).toFixed(1)} ms -> ${pick}`, chosenMs);
    }
  };

  // ── statistics (the evidence for "why is it slow") ────────────────────────────────────────
  const win = { track: 0, rvfc: 0, processed: 0, busy: 0, e2e: [] as number[], t0: performance.now(), droppedBase: 0 };
  const recent = { cam: 0, t0: performance.now() };
  let latencyKind: LatencyKind = "arrival";

  window.setInterval(() => {
    const dt = (performance.now() - recent.t0) / 1000;
    const fps = recent.cam / Math.max(dt, 0.001);
    recent.cam = 0;
    recent.t0 = performance.now();
    if (!preview) return; // Rust shows the low-light note on the overlay from `hand_stats`
    const hint = cameraHint(fps);
    fpsNote.textContent = hint ?? "";
    fpsNote.classList.toggle("on", hint !== null);
  }, 2000);

  window.setInterval(async () => {
    const dt = (performance.now() - win.t0) / 1000;
    const dropped = video.getVideoPlaybackQuality?.().droppedVideoFrames ?? 0;
    const stats = {
      source,
      cam_fps_track: win.track / dt,
      cam_fps_rvfc: win.rvfc / dt,
      proc_fps: win.processed / dt,
      driver_fps: cam.driverFps,
      busy_skips: win.busy,
      dropped: dropped - win.droppedBase,
      e2e_p50: win.e2e.length ? percentile(win.e2e, 50) : 0,
      e2e_p95: win.e2e.length ? percentile(win.e2e, 95) : 0,
      latency_kind: latencyKind,
      page: document.visibilityState,
      exposure: cam.exposure,
      power: await batteryText(),
      covered,
    };
    win.track = win.rvfc = win.processed = win.busy = 0;
    win.e2e = [];
    win.droppedBase = dropped;
    win.t0 = performance.now();
    void invoke("hand_stats", { stats }).catch(() => {});
  }, 5000);

  // ── one frame ─────────────────────────────────────────────────────────────────────────────
  const process = (src: CanvasImageSource, w: number, h: number, captureEpoch: number | null, kind: LatencyKind) => {
    frameIdx++;
    const now = Date.now();
    if (building) return;
    if (inflight) {
      win.busy++;
      return;
    }
    if (!shouldProcessFrame(frameIdx, now, lastHandMs, gpuBusy)) return;
    const [brightness, variance] = sampleCoverage(src, w, h);
    if (isLikelyCovered(brightness, variance)) {
      if (coveredSinceMs === null) coveredSinceMs = now;
      covered = now - coveredSinceMs >= COVERED_DEBOUNCE_MS;
    } else {
      coveredSinceMs = null;
      covered = false;
    }
    const t = performance.now();
    const res = landmarker.detectForVideo(src as TexImageSource, t);
    const infer = performance.now() - t;
    calibrate(infer);
    const hand = res.landmarks?.[0];
    if (hand) lastHandMs = now;
    if (preview) draw(src, w, h, hand);
    latencyKind = captureEpoch === null ? "arrival" : kind;
    const cap = captureEpoch ?? now;
    win.processed++;
    win.e2e.push(Math.max(0, Date.now() - cap));
    inflight = true;
    invoke("hand_frame", { frame: toPayload(hand, now, infer, cap) })
      .catch(() => {})
      .finally(() => {
        inflight = false;
      });
  };

  // ── frame sources ─────────────────────────────────────────────────────────────────────────
  const onRvfc = (_now: number, md: VideoFrameCallbackMetadata) => {
    win.rvfc++;
    recent.cam++;
    if (source === "rvfc") {
      const cap = md.captureTime != null ? performance.timeOrigin + md.captureTime : null;
      process(video, video.videoWidth || 640, video.videoHeight || 480, cap, "capture");
    }
    video.requestVideoFrameCallback(onRvfc);
  };
  if (needVideo && "requestVideoFrameCallback" in video) video.requestVideoFrameCallback(onRvfc);

  if (MSTP) {
    const reader = new MSTP({ track: cam.track, maxBufferSize: 1 }).readable.getReader();
    let offset: number | null = null;
    void (async () => {
      for (;;) {
        const { value: frame, done } = await reader.read();
        if (done || !frame) break;
        win.track++;
        if (source === "track") recent.cam++;
        try {
          if (source === "track") {
            const tsMs = frame.timestamp / 1000;
            // The first frame is assumed fresh, so absolute latency here is a lower bound.
            if (offset === null) offset = performance.now() - tsMs;
            process(frame as unknown as CanvasImageSource, frame.displayWidth, frame.displayHeight, performance.timeOrigin + tsMs + offset, "track-lower-bound");
          }
        } finally {
          frame.close();
        }
      }
    })();
  }
}

listen<HudState>("hand:state", (e) => renderState(e.payload));
boot().catch(() => {});
window.addEventListener("beforeunload", () => {
  (video.srcObject as MediaStream | null)?.getTracks().forEach((t) => t.stop());
});
