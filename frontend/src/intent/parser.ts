/**
 * Local intent parser — regex-based command matching with phonetic fallback.
 *
 * Works without a backend. Parses the STT transcript into a structured
 * intent that can be executed locally (open app, open URL, search, etc.)
 *
 * 3-TIER RESOLUTION: All "open <app>" commands are sent to Rust as
 * `open_app` intents. The Rust command_executor resolves them in order:
 *   Tier 1: Is the app already running? → Focus its window
 *   Tier 2: Is the app installed? → Launch the native app (Store/PWA/Win32)
 *   Tier 3: Is there a URL fallback? → Open in browser
 *   Tier 4: Not found → Speak error
 *
 * The frontend does NOT short-circuit to browser URLs. Rust owns the
 * full resolution chain so native apps, Store apps, and PWAs are always
 * preferred over browser tabs.
 *
 * PHONETIC MATCHING: If the STT transcript contains a word that sounds like
 * a known app name (e.g. "gamail" → "gmail"), the phonetic matcher corrects
 * it before sending to Rust. This handles mispronunciations and accents
 * without requiring any model training.
 *
 * If no local intent matches, returns {action:"unknown"} so the caller
 * can fall through to the remote backend (if available).
 */

export type Intent =
  | { action: "open_app"; target: string }
  | { action: "open_url"; target: string; url: string }
  | { action: "close_app"; target: string }
  | { action: "whatsapp_chat"; contact: string }
  | { action: "open_architect" }
  | { action: "open_settings" }
  | { action: "search"; query: string }
  | { action: "analyse_repo"; owner?: string; repo: string }
  | { action: "analyse_pr"; owner?: string; repo: string; pr_number: number }
  | { action: "analyse_latest_pr"; owner?: string; repo: string; author?: string }
  | { action: "check_branch"; owner?: string; repo: string; author?: string }
  | { action: "media_play_pause" }
  | { action: "media_next" }
  | { action: "media_previous" }
  | { action: "media_stop" }
  | { action: "github_command"; command: unknown }
  | { action: "order_food"; query: string; restaurant?: string }
  | { action: "search_product"; query: string }
  | { action: "send_whatsapp_message"; contact: string; message: string }
  | { action: "need_more_info"; prompt: string }
  | { action: "enter_ghostwriter"; contact?: string }
  | { action: "screen_click"; ordinal: number }
  | { action: "screen_read"; ordinal: number }
  | { action: "browser_tab"; index: number }
  | { action: "greeting"; reply: string }
  | { action: "nlu_result"; intent: string; slots: unknown; confidence: number }
  | { action: "unknown"; raw: string };

/**
 * URL map for explicit "open <app> in browser" commands ONLY.
 * This is NOT used for regular "open <app>" — those go to Rust which
 * checks for native apps/Store apps/PWAs first, then falls back to URLs.
 * Kept here so the frontend can handle "open gmail in browser" without
 * a round-trip to Rust.
 */
const BROWSER_FORCE_URL_MAP: Record<string, string> = {
  gmail: "https://mail.google.com",
  "google mail": "https://mail.google.com",
  youtube: "https://www.youtube.com",
  "you tube": "https://www.youtube.com",
  github: "https://github.com",
  "git hub": "https://github.com",
  twitter: "https://twitter.com",
  x: "https://x.com",
  facebook: "https://facebook.com",
  instagram: "https://instagram.com",
  reddit: "https://reddit.com",
  linkedin: "https://linkedin.com",
  whatsapp: "https://web.whatsapp.com",
  "whatsapp web": "https://web.whatsapp.com",
  spotify: "https://open.spotify.com",
  netflix: "https://netflix.com",
  amazon: "https://amazon.com",
  "google drive": "https://drive.google.com",
  "google docs": "https://docs.google.com",
  "google sheets": "https://sheets.google.com",
  "google slides": "https://slides.google.com",
  "google maps": "https://maps.google.com",
  "google calendar": "https://calendar.google.com",
  "google translate": "https://translate.google.com",
  "google photos": "https://photos.google.com",
  "google news": "https://news.google.com",
  "google meet": "https://meet.google.com",
  "google chat": "https://chat.google.com",
  "google play": "https://play.google.com",
  "google play store": "https://play.google.com",
  "play store": "https://play.google.com",
  "app store": "https://apps.apple.com",
  "mac app store": "https://apps.apple.com",
  chatgpt: "https://chat.openai.com",
  "chat gpt": "https://chat.openai.com",
  "open ai": "https://chat.openai.com",
  openai: "https://chat.openai.com",
  claude: "https://claude.ai",
  figma: "https://figma.com",
  notion: "https://notion.so",
  slack: "https://slack.com",
  discord: "https://discord.com/app",
  twitch: "https://twitch.tv",
  "stack overflow": "https://stackoverflow.com",
  stackoverflow: "https://stackoverflow.com",
  wikipedia: "https://wikipedia.org",
  chat: "https://chat.google.com",
  maps: "https://maps.google.com",
  translate: "https://translate.google.com",
  "my drive": "https://drive.google.com",
  calendar: "https://calendar.google.com",
};

// ─── Phonetic matching (DoubleMetaphone) ───────────────────────────────────
//
// When Whisper mishears a word (e.g. "gamail" instead of "gmail"), the
// phonetic matcher corrects it. DoubleMetaphone converts words to their
// phonetic root codes — words that sound the same get the same code.
//
// Example: "gmail" → "KML", "gamail" → "KML" → match!
//
// This is a compact implementation of Lawrence Philips' Double Metaphone
// algorithm. No external dependency needed.

/**
 * Stop words — common verbs, prepositions, and everyday English words that
 * should NEVER be corrected to app names by the phonetic matcher.
 * Without this, "open" matches "openai" phonetically (both → "APN").
 */
const PHONETIC_STOP_WORDS = new Set([
  // Command verbs
  "open", "close", "launch", "start", "run", "stop", "play", "pause",
  "search", "find", "show", "hide", "bring", "fire", "shut", "turn",
  "set", "get", "go", "put", "take", "make", "give", "send", "tell",
  // Common English words that sound like app names
  "up", "down", "in", "out", "on", "off", "to", "for", "the", "a", "an",
  "my", "your", "this", "that", "it", "is", "and", "or", "not", "do",
  "me", "we", "he", "she", "all", "can", "will", "new", "now", "please",
  "ok", "hey", "hi", "yes", "no", "maybe", "just", "very", "well",
  // Commonly confused with app names
  "check", "tab", "page", "window", "app", "site", "web", "mail",
]);

/** Known app names for phonetic matching. Keys are canonical names. */
const KNOWN_APPS: string[] = [
  "gmail", "youtube", "github", "google", "chrome", "brave", "firefox",
  "twitter", "instagram", "facebook", "reddit", "linkedin", "whatsapp",
  "netflix", "amazon", "wikipedia", "twitch", "spotify", "discord",
  "slack", "notion", "figma", "chatgpt", "claude", "gemini",
  "drive", "docs", "sheets", "slides", "maps", "calendar", "translate",
  "photos", "meet", "chat", "notepad", "calculator", "explorer",
  "terminal", "powershell", "paint", "settings", "outlook", "word",
  "excel", "powerpoint", "vscode", "code", "steam", "zoom", "teams",
  "skype", "telegram", "edge", "opera", "safari", "blender", "unity",
  "docker", "postman", "obs", "vlc",
];

/** Multi-word app names that should be matched as phrases. */
const KNOWN_PHRASES: string[] = [
  "google mail", "you tube", "git hub", "whatsapp web", "google drive",
  "google docs", "google sheets", "google slides", "google maps",
  "google calendar", "google translate", "google photos", "google news",
  "google meet", "google chat", "google play", "play store", "app store",
  "mac app store", "chat gpt", "open ai", "stack overflow", "visual studio code",
  "file explorer", "command prompt", "task manager", "control panel",
  "windows terminal", "google gemini",
];

/** Pre-computed metaphone codes for known apps (built once on module load). */
const APP_PHONETIC_INDEX: Map<string, string[]> = new Map(
  [...KNOWN_APPS, ...KNOWN_PHRASES].map((app) => [app, doubleMetaphone(app)]),
);

/**
 * Correct a single word to the closest known app name using phonetic matching.
 * Returns the corrected word, or the original if no match is close enough.
 */
function phoneticCorrectWord(word: string): string {
  if (!word || word.length < 2) return word;
  // Never correct common English words — they aren't app names.
  if (PHONETIC_STOP_WORDS.has(word.toLowerCase())) return word;
  const codes = doubleMetaphone(word);
  let bestMatch: string | null = null;
  let bestScore = 0;

  for (const [app, appCodes] of APP_PHONETIC_INDEX) {
    // Compare all code combinations (primary×primary, primary×secondary, etc.)
    for (const code of codes) {
      for (const appCode of appCodes) {
        if (!code || !appCode) continue;
        if (code === appCode) {
          // Exact phonetic match — prefer shorter app names (closer to the word)
          const score = word.length === app.length ? 3 : 2;
          if (score > bestScore) {
            bestScore = score;
            bestMatch = app;
          }
        } else if (code.length > 1 && appCode.length > 1) {
          // Partial match — first 2 chars of metaphone code match
          if (code.substring(0, 2) === appCode.substring(0, 2)) {
            // Also check Levenshtein distance for extra confidence
            const dist = levenshtein(word, app);
            if (dist <= 2 && dist < word.length / 2) {
              const score = 1;
              if (score > bestScore) {
                bestScore = score;
                bestMatch = app;
              }
            }
          }
        }
      }
    }
  }

  // Only correct if we found a confident match (score >= 2 = exact phonetic match)
  if (bestMatch && bestScore >= 2) {
    return bestMatch;
  }
  return word;
}

/**
 * Correct a phrase (possibly multi-word) to the closest known app name.
 * Tries multi-word phrase matching first, then word-by-word correction.
 */
function phoneticCorrectPhrase(phrase: string): string {
  const trimmed = phrase.trim().toLowerCase();

  // 1. Try exact match first (most common case — Whisper got it right)
  if (APP_PHONETIC_INDEX.has(trimmed)) {
    return trimmed;
  }

  // 2. Try multi-word phrase phonetic match
  const phraseCodes = doubleMetaphone(trimmed);
  for (const [app, appCodes] of APP_PHONETIC_INDEX) {
    if (app.includes(" ")) {
      for (const code of phraseCodes) {
        for (const appCode of appCodes) {
          if (code && appCode && code === appCode) {
            return app;
          }
        }
      }
    }
  }

  // 3. Try word-by-word correction
  const words = trimmed.split(/\s+/);
  let anyCorrected = false;
  const correctedWords = words.map((w) => {
    const cw = phoneticCorrectWord(w);
    if (cw !== w) {
      console.log(`[NEXUS] phonetic match: "${w}" → "${cw}"`);
      anyCorrected = true;
    }
    return cw;
  });

  if (anyCorrected) {
    const result = correctedWords.join(" ");
    // Check if the corrected phrase is a known multi-word app
    if (APP_PHONETIC_INDEX.has(result)) {
      return result;
    }
    return result;
  }

  return trimmed;
}

// ─── Double Metaphone algorithm (Lawrence Philips, 2000) ───────────────────
// Compact implementation — converts words to phonetic root codes.
// Words that sound the same produce the same code, regardless of spelling.

function doubleMetaphone(word: string): string[] {
  const w = word.toUpperCase().replace(/[^A-Z]/g, "");
  if (w.length === 0) return ["", ""];

  const primary: string[] = [];
  const secondary: string[] = [];
  let current = 0;
  const length = w.length;

  // Handle silent initials
  if (w.match(/^(GN|KN|PN|WR|PS)/)) {
    current = 1;
  }

  // Handle 'X' at start → 'S'
  if (w[0] === "X") {
    add("S");
    add("Z");
    current = 1;
  }

  while (current < length && (primary.length < 4 || secondary.length < 4)) {
    const c = w[current];
    const prev = current > 0 ? w[current - 1] : "";
    const next = current + 1 < length ? w[current + 1] : "";
    const next2 = current + 2 < length ? w[current + 2] : "";

    switch (c) {
      case "A":
      case "E":
      case "I":
      case "O":
      case "U":
      case "Y":
        if (current === 0) { add("A"); }
        current++;
        break;

      case "B":
        add("P");
        current += (c === "B" && next === "B") ? 2 : 1;
        break;

      case "C":
        if (current > 0 && w.slice(current, current + 2) === "CH") {
          add("X");
          current += 2;
        } else if (current > 0 && w.slice(current, current + 2) === "CI") {
          add("S");
          current += 2;
        } else if (current > 0 && w.slice(current, current + 2) === "CE") {
          add("S");
          current += 2;
        } else if (w.slice(current, current + 3) === "CIA") {
          add("X");
          current += 3;
        } else if (next === "H") {
          add("X");
          current += 2;
        } else if (next === "I" || next === "E" || next === "Y") {
          add("S");
          current += 2;
        } else {
          add("K");
          current += 1;
        }
        break;

      case "D":
        if (next === "G" && (next2 === "I" || next2 === "E" || next2 === "Y")) {
          add("J");
          current += 3;
        } else {
          add("T");
          current += (next === "D") ? 2 : 1;
        }
        break;

      case "F":
        add("F");
        current += (next === "F") ? 2 : 1;
        break;

      case "G":
        if (next === "H" && current > 0 && !isVowel(prev)) {
          add("K");
          current += 2;
        } else if (next === "H" && current + 2 < length && !isVowel(next2)) {
          add("K");
          current += 2;
        } else if (current > 0 && w.slice(current, current + 2) === "GN") {
          add("N");
          current += 2;
        } else if (next === "I" || next === "E" || next === "Y") {
          add("J");
          current += 2;
        } else {
          add("K");
          current += (next === "G") ? 2 : 1;
        }
        break;

      case "H":
        if (current === 0 && isVowel(next)) {
          add("H");
          current += 2;
        } else if (current > 0 && !isVowel(prev)) {
          add("H");
          current += 1;
        } else {
          current += 1;
        }
        break;

      case "J":
        add("J");
        current += (next === "J") ? 2 : 1;
        break;

      case "K":
        add("K");
        current += (next === "K") ? 2 : 1;
        break;

      case "L":
        add("L");
        current += (next === "L") ? 2 : 1;
        break;

      case "M":
        add("M");
        current += (next === "M") ? 2 : 1;
        break;

      case "N":
        add("N");
        current += (next === "N") ? 2 : 1;
        break;

      case "P":
        if (next === "H") {
          add("F");
          current += 2;
        } else {
          add("P");
          current += (next === "P") ? 2 : 1;
        }
        break;

      case "Q":
        add("K");
        current += (next === "Q") ? 2 : 1;
        break;

      case "R":
        add("R");
        current += (next === "R") ? 2 : 1;
        break;

      case "S":
        if (next === "H") {
          add("X");
          current += 2;
        } else if (next === "I" && (next2 === "O" || next2 === "A")) {
          add("X");
          current += 3;
        } else {
          add("S");
          current += (next === "S") ? 2 : 1;
        }
        break;

      case "T":
        if (next === "H") {
          add("0"); // 'TH' → theta
          current += 2;
        } else if (next === "I" && (next2 === "O" || next2 === "A")) {
          add("X");
          current += 3;
        } else {
          add("T");
          current += (next === "T") ? 2 : 1;
        }
        break;

      case "V":
        add("F");
        current += (next === "V") ? 2 : 1;
        break;

      case "W":
        if (current === 0 && isVowel(next)) {
          add("A");
        }
        current += 1;
        break;

      case "X":
        add("K");
        add("S");
        current += (next === "X") ? 2 : 1;
        break;

      case "Z":
        add("S");
        current += (next === "Z") ? 2 : 1;
        break;

      default:
        current += 1;
        break;
    }
  }

  function add(c: string) {
    if (primary.length < 4) primary.push(c);
    if (secondary.length < 4) secondary.push(c);
  }

  function isVowel(ch: string): boolean {
    return "AEIOUY".includes(ch);
  }

  const p = primary.join("");
  const s = secondary.join("");
  return [p, s === p ? "" : s];
}

/** Levenshtein edit distance — used as a secondary check for phonetic matching. */
function levenshtein(a: string, b: string): number {
  const m = a.length;
  const n = b.length;
  if (m === 0) return n;
  if (n === 0) return m;

  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0));
  for (let i = 0; i <= m; i++) dp[i][0] = i;
  for (let j = 0; j <= n; j++) dp[0][j] = j;

  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      dp[i][j] = Math.min(
        dp[i - 1][j] + 1,
        dp[i][j - 1] + 1,
        dp[i - 1][j - 1] + cost,
      );
    }
  }

  return dp[m][n];
}

/**
 * Parse a transcript into a structured intent.
 *
 * @param transcript - The STT transcript text (e.g. "open gmail")
 * @returns A structured Intent, or {action:"unknown"} if no match
 */
export function parseIntent(transcript: string): Intent {
  const text = transcript.trim().toLowerCase().replace(/\s+/g, " ");

  // --- Open Architecture Mapper ---
  // "open architecture mapper", "open architect", "open architecture map"
  // "launch architecture mapper", "show architecture", "open architect window"
  if (/^(?:open|launch|start|show|bring\s+up|pull\s+up)\s+(?:architecture|architect)(?:\s+(?:mapper|map|window|mapper\s+window))?$/i.test(text)) {
    return { action: "open_architect" };
  }
  // Also catch "open the architect" / "open the architecture mapper"
  if (/^(?:open|launch|start|show)\s+the\s+(?:architecture|architect)(?:\s+(?:mapper|map|window))?$/i.test(text)) {
    return { action: "open_architect" };
  }

  // --- Open Settings / Command Center ---
  // "open settings" / "show settings" / "open command center" / "open preferences"
  // "configure NEXUS" / "NEXUS settings" / "open config" / "show preferences"
  if (/^(?:open|launch|start|show|bring\s+up|pull\s+up|give\s+me|show\s+me)\s+(?:me\s+)?(?:the\s+)?settings?$/i.test(text)) {
    return { action: "open_settings" };
  }
  if (/^(?:open|launch|start|show|bring\s+up|pull\s+up|give\s+me|show\s+me)\s+(?:me\s+)?(?:the\s+)?command\s+center$/i.test(text)) {
    return { action: "open_settings" };
  }
  if (/^(?:open|launch|start|show|bring\s+up|pull\s+up|give\s+me|show\s+me)\s+(?:me\s+)?(?:the\s+)?preferences?$/i.test(text)) {
    return { action: "open_settings" };
  }
  if (/^(?:open|launch|start|show|bring\s+up|pull\s+up|give\s+me|show\s+me)\s+(?:me\s+)?(?:the\s+)?(?:config|configuration)$/i.test(text)) {
    return { action: "open_settings" };
  }
  if (/^configure\s+nexus$/i.test(text)) {
    return { action: "open_settings" };
  }
  if (/^nexus\s+(?:settings?|config|configuration|preferences?|command\s+center)$/i.test(text)) {
    return { action: "open_settings" };
  }
  // Bare words (Intel SST mic truncation)
  if (/^(?:settings|preferences|config|configuration)$/i.test(text)) {
    return { action: "open_settings" };
  }

  // --- Media Control (MPRIS D-Bus / System Keys) ---
  if (/^(?:pause|pause\s+music|pause\s+media|play|resume|resume\s+music|play\s*[\/\s]*pause|toggle\s+media)$/i.test(text)) {
    return { action: "media_play_pause" };
  }
  if (/^(?:next|next\s+song|next\s+track|skip|skip\s+song|skip\s+track)$/i.test(text)) {
    return { action: "media_next" };
  }
  if (/^(?:previous|previous\s+song|previous\s+track|prev|prev\s+song|go\s+back\s+a\s+song)$/i.test(text)) {
    return { action: "media_previous" };
  }
  if (/^(?:stop\s+music|stop\s+media|stop\s+playback)$/i.test(text)) {
    return { action: "media_stop" };
  }

  // --- "open <something>" / "launch <something>" / etc. ---
  // Matches: "open gmail", "open notepad", "open calculator", "open youtube"
  //          "launch spotify", "start calculator", "run powershell"
  //          "fire up chrome", "bring up notepad", "show calculator"
  // All "open" commands go through the 3-tier resolution in Rust:
  //   running → installed → browser fallback → not found
  const openMatch = text.match(
    /^(?:open|launch|start|run|fire\s+up|bring\s+up|show|pull\s+up|go\s+to|visit|browse\s+to|navigate\s+to)\s+(.+)$/i,
  );
  if (openMatch) {
    const target = openMatch[1].trim();

    // Strip trailing "app" or "application" — "open gmail app" → "gmail"
    const cleaned = target.replace(/\s+(?:app|application|for\s+me)$/i, "");

    // ESCAPE HATCH: "open gmail in browser" / "open gmail website" / "open gmail site"
    // Forces browser URL instead of native app. Lets user choose browser version
    // even when a native app/PWA is installed.
    const browserForceMatch = cleaned.match(
      /^(.+?)\s+(?:in\s+(?:the\s+)?browser|website|site|on\s+(?:the\s+)?web|web\s+version)$/i,
    );
    if (browserForceMatch) {
      const appName = browserForceMatch[1].trim();
      const url = BROWSER_FORCE_URL_MAP[appName];
      if (url) {
        return { action: "open_url", target: appName, url };
      }
      // Unknown app + "in browser" → try constructing a URL
      if (appName.includes(".") && !appName.includes(" ")) {
        const fullUrl = appName.startsWith("http") ? appName : `https://${appName}`;
        return { action: "open_url", target: appName, url: fullUrl };
      }
      // Fall through to open_app — Rust will URL-fallback if no native app
    }

    // Strip trailing "website"/"site" if not matched above (e.g. "open gmail website" without "in")
    const cleanedNoSite = cleaned.replace(/\s+(?:website|site)$/i, "");

    // If it looks like a URL (has a dot, no spaces), open directly
    if (cleanedNoSite.includes(".") && !cleanedNoSite.includes(" ")) {
      const fullUrl = cleanedNoSite.startsWith("http") ? cleanedNoSite : `https://${cleanedNoSite}`;
      return { action: "open_url", target: cleanedNoSite, url: fullUrl };
    }

    // PHONETIC CORRECTION: If Whisper misheard the app name (e.g. "gamail"
    // instead of "gmail"), correct it using DoubleMetaphone matching.
    // This runs AFTER regex matching but BEFORE sending to Rust, so the
    // Rust resolver receives the canonical app name.
    const corrected = phoneticCorrectPhrase(cleanedNoSite);
    if (corrected !== cleanedNoSite) {
      console.log(`[NEXUS] phonetic correction: "${cleanedNoSite}" → "${corrected}"`);
    }

    // All "open" commands go to Rust as open_app.
    // Rust handles: running check → installed check → URL fallback.
    // This ensures native apps, Store apps, and PWAs are preferred over browser tabs.
    return { action: "open_app", target: corrected };
  }

  // --- "close <app>" / "quit <app>" / "exit <app>" ---
  const closeMatch = text.match(
    /^(?:close|quit|exit|kill|shut\s+down|shut)\s+(.+)$/i,
  );
  if (closeMatch) {
    const target = closeMatch[1].trim();
    if (target && !target.match(/^(nexus|the app)$/i)) {
      return { action: "close_app", target };
    }
  }

  // --- "open chat with <name>" / "chat with <name>" (chat-open ONLY) ---
  // Anything send-shaped ("send message to X", anything with "saying",
  // "message <X> saying ...") is NOT claimed here — it belongs to the Rust
  // orchestrator (NeedMoreInfo / SendWhatsAppMessage), which asks for
  // missing slots and gates the send. Claiming it here caused "Ok sir"
  // + a garbage contact ("mommy in WhatsApp") with no question asked.
  if (/\bsaying\b/i.test(text)) {
    return { action: "unknown", raw: text };
  }
  const whatsappMatch = text.match(
    /^(?:open\s+(?:my\s+)?chat\s+with|chat\s+with|message|whatsapp|open\s+whatsapp\s+chat\s+with)\s+(.+?)(?:\s+(?:on|in)\s+whatsapp)?$/i,
  );
  if (whatsappMatch) {
    const contact = whatsappMatch[1].trim();
    // "send ..." leftovers are send-shaped, not chat opens — defer to Rust.
    if (contact && !/^send\b/i.test(contact)) {
      return { action: "whatsapp_chat", contact };
    }
  }

  // --- "search for <query>" ---
  // Matches: "search for cats", "google cats", "look up cats", "find cats"
  //          "search cats on youtube", "youtube cats", "spotify play <song>"
  const searchMatch = text.match(
    /^(?:search\s+for|search|google|look\s+up|find\s+me|find|look\s+for)\s+(.+)$/i,
  );
  if (searchMatch) {
    const query = searchMatch[1].trim();
    return {
      action: "search",
      query,
    };
  }

  // --- "play <song> on spotify" / "play <song>" ---
  const spotifyMatch = text.match(
    /^(?:play|put\s+on)\s+(.+?)(?:\s+on\s+spotify)?$/i,
  );
  if (spotifyMatch) {
    const query = spotifyMatch[1].trim();
    // If they said "on spotify" or just "play <song>", search on YouTube
    // (most universal) — the Rust executor can handle spotify_play too
    return {
      action: "search",
      query: `${query} on spotify`,
    };
  }

  // --- "close <app>" / "quit <app>" / "exit <app>" ---
  // TODO: These could be handled by Rust in the future. For now, treat as
  // unknown so the backend can handle them if available.
  // const closeMatch = text.match(/^(?:close|quit|exit|kill)\s+(.+)$/i);

  // --- No local intent matched ---
  return { action: "unknown", raw: transcript };
}
