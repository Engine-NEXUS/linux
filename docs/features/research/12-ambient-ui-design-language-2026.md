# Ambient UI & Design Language — What Actually Ships in 2026

> **Part of the 2026-10 competitive & platform audit.**
> Decision record: [`docs/features/62-competitive-and-platform-audit-2026-10.md`](../62-competitive-and-platform-audit-2026-10.md)
> Companion compendium (identical content, canonical home):
> [`NEXUS-PAPERS → research/2026-10-competitive-audit/`](https://github.com/Engine-NEXUS/NEXUS-PAPERS)
>
> This file is **evidence**. It records what was verified on 2026-10-02, from
> where, and what could not be verified. It is not a work order — see the spec above
> for that.

> **What this covers:** How production assistants signal presence vs intrusion, the collapse-to-small-element rules, M3 Expressive motion specs, and documented redesign failures.


**Date:** 2026-10-02
**Scope:** How production assistants signal presence vs intrusion, the
"collapse to a small persistent element" pattern, concrete motion specs, and a
catalogue of documented redesign failures relevant to an always-on desktop orb.

---

## 1. Presence: "active but not demanding attention"

### Google Gemini Live — changed three times in four months, converging on small

| Date | Change |
|---|---|
| 2026-02-28 | Exiting fullscreen gives a **floating pill**, draggable, over other apps. Collapses to a **circle**. Tap = expand, swipe down = dismiss (chathead-like). |
| 2026-04-07 | Pill → **floating interface**: waveform centred, flanked by screen-share and keyboard-to-exit, captions top-right. *"Will condense into a smaller circle than before as you start navigating."* Fullscreen removed. |
| 2026-04-19 | In-app variant: prompt box replaced by **pill with waveform**; top bar becomes "Live with Gemini" + transcript. |
| 2026-05-19 | I/O: **fullscreen killed permanently** for the main flow. *"Waveform housed in a centered pill,"* flanking controls. |
| 2026-06-17 | Gemini **overlay** gets Android **Bubbles** — *"Gemini is still available while you multitask. Tap to expand. Drag to move or dismiss."* Bubble shows **only the spark logo**. |
| 2026-09-04 | Bubbles reaches **stable** Google app. |

Key presence signals Google uses:
- **Live waveform reacting to voice amplitude** — *"speak softly and the animation
  stays small; speak louder and it expands."*
- **Transcript stays visible** above the controls even when collapsed.
- **Hypnagogic launch** — *"the launch animation feels intentionally slowed."*
- **Gauzy, ethereal overlay** — *"almost as if Google is trying to portray that
  Gemini is alive."*

### Apple

- Siri with Apple Intelligence produces an **edge-to-edge rainbow shimmer** that
  *"wraps around the edges of your device's screen"* and briefly distorts
  everything. Requires Apple Intelligence hardware.
- **Silent A/B split**: devices without it still get the old bottom orb, which
  generated a lot of confusion and hundreds of Apple Community threads asking
  "how do I get screen to glow around the edge?"
- HIG explicitly designs for **loss of focus**: *"when a window loses focus on
  the Mac or iPad, Liquid Glass shifts its appearance and visually recedes to
  guide attention."*

### Apple Live Captions — the cleanest ambient precedent

Floating pop-up window, auto-dismissable, with a **four-button control strip**
(minimize / pause / mic / maximize), and an explicit **"Idle opacity" slider** in
Settings → Accessibility. Fully on-device, works with the phone muted.

### Microsoft 365 Copilot — "present but not imposing"

2026-05-28, Jon Friedman (Chief Design Officer). The stated goal is explicitly to
**strip** personality: *"how little color it has,"* panels *"which collapse when
not in use,"* and a *"prompt surface that changes size and reveals new functions
as you type."*

Consumer Copilot remains *"still bright, colorful and (occasionally) blobby"* —
**two Copilots with different personalities at once**, which Engadget called
evidence that *"Microsoft's AI strategy is very much in flux."*

Windows voice: long-press Copilot key → *"voice controller, a small window that
stays on screen for multitasking."*

**"Hey Copilot"** is off by default, opt-in, **English only**, Windows desktop
only, cannot start when the screen is locked or another app has audio, and
auto-ends after **1 minute of inactivity**. Microsoft explicitly warns:
*"Language and accent variations may cause accidental activation"* and *"Voice
chats in Copilot may pick up background noise, which can keep the voice chat
active even though you aren't speaking."* — **directly relevant to our wake-word
false-trigger work.**

### Amazon Alexa

A blue light indicates engagement, but the Verge hands-on found the timeout is
**~30 seconds** and complained it should persist longer. Alexa+ Echo Show 8/11
(2025 redesign): *"It also lacks a real purpose and refinement in the user
interface."* Alexa+ still absent in the UK as of Jan 2026.

### Raycast — the opposite bet

**No ambient presence at all.** Keyboard-first, summoned, transient. Their
2026 work is AI *in the flow* (Quick AI, Screen Awareness v0.71, 2026-08-19),
not a persistent agent surface.

Notably, their changelog records a bug worth noting: *"AI Chat: Fixed chats that
stayed stuck looking busy after the agent had finished"* — a **false-busy** bug,
the mirror image of a false-idle problem.

### ChatGPT

Ships **three distinct presentations**: integrated in-chat, **"a floating voice
orb,"** or separate full-screen.
- **2026-08-31** — Live voice content on iPhone Lock Screen and in the Dynamic
  Island (`Settings → Voice → Background conversations`).
- **2026-09-29** — **"dots"**: always-on agents with *"its own cloud computer."*
  Pro + Business Premium; excludes EEA/CH/UK; Enterprise beta, off by default.
  Created in desktop app or web only.
- ⚠️ **No documentation of any persistent visual surface for "dots"** was found.

---

## 2. The collapse pattern, with documented rules

### Android App Bubbles — now the platform-level version

Shipped 2026-06-16, Pixel 6+. Google's own framing: *"It keeps everything you
need in reach but out of the way."*

| Action | Mechanism |
|---|---|
| Create | Long-press app icon → Bubble |
| Expand | Tap the floating bubble |
| Minimize | Tap again, or tap outside the bubble window |
| Bar | Tap the bubble bar to find all bubbles; icons switch; touch-and-hold to move |
| Large screens | Dedicated bubble bar docked in the taskbar; resizable/maximizable |
| Dismiss | Touch and hold → close |

### Apple Live Activities — the most rigorously specified collapse in existence

| Rule | Detail |
|---|---|
| Scope gate | *"Offer Live Activities for tasks and events that have a defined beginning and end."* |
| Hard duration cap | *"short to medium duration activities that don't exceed eight hours"* |
| Compact | One active activity; compact leading + trailing flanking the camera cutout |
| Minimal | Multiple; one attached to the Island, one **detached, circular/oval** |
| Expanded | Touch-and-hold |
| Alerting | *"Alert people only for essential updates that require their attention"* |
| Interactivity | *"prefer limiting it to a single element"* |
| Margins | 14 pt added to all content |
| Snugness | *"Be as narrow as possible with no wasted space."* |
| **Don't fade to logo** | *"Avoid reverting to purely just a logo here, and think about how your session can continue to convey information even in this tiny state."* Apple's own Timer shows **remaining time, not a static icon**. |
| Animation | *"Use animations sparingly, and only to bring attention to content updates."* |
| Prohibited | *"Don't add elements to your app that draw attention to the Dynamic Island."* |

⚠️ The Dynamic Island px dimensions floating around (767×108, r=132) come from an
**unsourced GitHub gist comment. Do not use.**

### A rule we can use directly

**A collapsed orb must still carry state.** Apple's instruction is explicit: do
not collapse to a bare logo; keep conveying information in the tiny state. Gemini
keeps the waveform in the collapsed circle; Apple's Timer shows remaining time.

For NEXUS: a 64 px idle orb should still show *something* — a live level meter, or
the state hue — not just a static glyph.

---

## 3. Motion design systems — concrete specs

### Material 3 Expressive — the only one with a published token system

M3 Expressive **replaced easing+duration with springs**. Google's note:
*"The easing and duration system… is **no longer maintained**."*

**Spring tokens** (spatial = overshoots; effects = no overshoot, use for colour
and opacity):

| Token | mass | stiffness | damping |
|---|---|---|---|
| `spatialFast` | 1 | 500 | 30 |
| `spatialMedium` | 1 | 350 | 28 |
| `spatialGentle` | 1 | 220 | 24 |
| `effectsFast` | 1 | 420 | 32 |
| `effectsMedium` | 1 | 280 | 28 |

**Web conversion table** (cubic-Bezier + duration) — this is what a Tauri/React
app actually needs:

| Spring | `cubic-bezier(...)` | Duration |
|---|---|---|
| Expressive fast spatial | `0.42, 1.67, 0.21, 0.90` | 350 ms |
| Expressive default spatial | `0.38, 1.21, 0.22, 1.00` | 500 ms |
| Expressive slow spatial | `0.39, 1.29, 0.35, 0.98` | 650 ms |
| Expressive fast effects | `0.31, 0.94, 0.34, 1.00` | **150 ms** |
| Expressive default effects | `0.34, 0.80, 0.34, 1.00` | **200 ms** |
| Expressive slow effects | `0.34, 0.88, 0.34, 1.00` | 300 ms |

**Speed-selection rule** (verbatim from Google's spec): *"Most motion should use
the default speed, while smaller elements may use fast and larger elements may
use slow."*

| Speed | Spatial use | Effects use |
|---|---|---|
| Default | Bottom sheet, expanded nav rail | Content opacity within a nav rail |
| Fast | Switches, buttons | Switch handle colour |
| Slow | Full-screen animations | Full-screen content refresh |

**Per-device variation:** *"the exact values of each token differ depending on if
the device is a wearable, phone, or tablet."*

Legacy M3 easing/duration (still valid for transitions): emphasized
`cubic-bezier(0.2,0,0,1)`, emphasized-decelerate `cubic-bezier(0.05,0.7,0.1,1)`,
emphasized-accelerate `cubic-bezier(0.3,0,0.8,0.15)`. Durations:
short1-4 = 50/100/150/200 ms · medium1-4 = 250/300/350/400 ms ·
long1-4 = 450/500/550/600 ms · extra-long1-4 = 700/800/900/1000 ms.

**M3 shape scale:** `0 / 4 / 8 / 12 / 16 / 20 / 28 / 32 / 48 dp / fully rounded`.
Nesting rule: *"Outer radius − padding = inner radius"* (48 dp − 14 dp = 34 dp).

⚠️ **Caveat:** `m3.material.io` is JS-rendered and could not be loaded directly.
Specs came from a third-party text mirror generated 2026-09-28. **Verify against
the live page before shipping.**

### 🔴 The one number to adopt: avoid 0.2 Hz

Apple HIG Motion, verbatim:
> *"you want to avoid showing an oscillation that has a frequency of around
> **0.2 Hz** because people can be very sensitive to this frequency. If you need
> to show objects oscillating, aim to keep the amplitude low and consider making
> the content translucent."*

**Our `.orb` fallback at `styles.css:84-93` uses `breathe 3s`** — a 0.33 Hz
oscillation, inside the danger band. `setup.css:47-50` defines a duplicate
`breathe` with the same 3 s period.

### Apple Liquid Glass

WWDC 2025-06-09, *"our broadest design update ever."* Motion designed together
with visuals; real-time rendering; lighting responds to device motion; touch gets
stronger emphasis than trackpad. **No numeric durations or curves published.**
HIG: *"All layout- and appearance-based animations automatically include
built-in easing… **You can't turn off or customize easing.**"*

Built-in accessibility: Reduced Transparency, Increased Contrast, Reduced Motion —
*"disables any elastic properties for the material."*

### Neural Expressive — observed delta only, no published spec

⚠️ **There is no published design spec for Neural Expressive.** No token set, no
durations, no curves, no colours, no haptics values anywhere — not on
m3.material.io, not in Gemini's resources, not in a design blog. Everything
circulating is one paragraph of marketing copy. What *is* verifiable is the
observed delta:

- **Typography** — *"the new, thinner typography used throughout. At first I
  thought it was a new font, but **it's the same Roboto… just thinned out
  considerably**."*
- **Icons** — thin outline style for Microphone, Camera, Gallery, File, Video,
  Screenshare, Live.
- **Colour** — blue-white gradient background; *"a colorful gradient temporarily
  surround the prompt bar"*.
- **Shape** — pill-shaped prompt box; pill-shaped current-item indicators.
- **Haptics** — *"slightly improved haptic feedback."* No values.

### Microsoft Fluent (the anti-Expressive position)

De-coloring, collapsing panels. No token values published.

### Android XR spatial UI

**14 dp minimum font size**, weight normal or higher. *"Spatial elevation"* via
**Orbiters** above panels on the Z-axis. Note: Android XR does **not** share raw
eye-tracking data with apps — it shows a generic hover effect.

---

## 4. Communicating state — the strongest research in the set

### Sidekick (UIST 2026, arXiv 2607.17527)

Covered in `10-competitive-audit-executive-summary.md` §1.2. The transferable design system:

1. **Ambient colour for status** — peripheral window shifts
   **green → yellow → orange → red** as *consecutive* errors accumulate.
   *"Color is preattentive: no reading required."*
2. **Lightweight sound for change** — spoken updates + **Foley clicks and
   keystrokes**, so eyes stay on the primary task.
3. **Spatial history** — synchronized audio-visual **replay** on return, colour
   gradient marking recently touched cells.
4. **Interrupt only for real decisions** — after **8 consecutive failed actions**,
   pause and request intervention. *"Otherwise it stays out of the way."*

⚠️ Important: Sidekick's ramp is **error count, not state**. Do not use colour to
encode state directly.

### Nielsen's feedback ladder — the timing budget

| Delay | User's mental state | Correct feedback |
|---|---|---|
| **< 0.1 s** | Feels like direct manipulation | None — the result *is* the feedback |
| **0.1–1 s** | Notices the lag, keeps flow | None, or a subtle transition |
| **1–10 s** | *"Attention strains at the leash"* | Spinner/skeleton, **after ~1 s of grace** |
| **10 s–1 min** | *"Attention lost; task-switching begins"* | **Percent-done + honest time estimate** |
| **1–10 min** | Leaves the screen | Persistent status surviving leave-and-return + interim results |
| **> 10 min** | User is gone | Step list, activity narration, interim artifacts, completion notification |

Underpinning: **0.1 s** = illusion of instantaneous · **1 s** = flow maintained ·
**10 s** = attention limit.

### Why "thinking" gets mistaken for "stuck"

Victor Yocco (ServiceNow), *Smashing Magazine*, **2026-05-13**:
> *"For thirty years, interface designers have relied on a single pattern to
> handle latency: **the spinner**… **AI agents introduce a new kind of wait
> time.** When an agent pauses for twenty seconds, it's not just downloading
> something; **it's thinking.**… If we use a basic spinning icon for this
> 'thinking time,' users get confused and anxious. **They watch a looping
> animation and can't tell if the system is stalled or crashed.**"*

Four named patterns:

| Pattern | Use case | Trust signal |
|---|---|---|
| **Living Breadcrumb** | Low-stakes background tasks | *"I am active, but I won't disturb you."* |
| **Dynamic Checklist** | High-stakes, variable time | *"I have a plan, and I am currently executing Step 2."* |
| **Thinking Toggle** | Expert tools | *"I have nothing to hide; here are my raw logs."* |
| **Audit Trail** | Post-task review | *"Here is the receipt of my work."* |

Also from Yocco:
- **Microcopy formula:** Action Word + Specific Item + Limits. Bad: *"Searching
  for flights…"* Good: *"Scanning **the prices on Lufthansa and United** **to find
  anything under $600**."*
- **Retire "Loading" and "Working."**
- **Design for partial success** — show `Flight booked: UA 492 [Success] /
  Hotel: Marriott [Success] / Car: Hertz [Failed — No inventory]` instead of a
  big red failure.
- **The expertise wall** — in field research, professionals *"tune out the
  interface entirely"* and *"judge the system based entirely on the final
  result."* *"This lack of transparency… is a primary barrier to adoption."*
  **If the explanation disappeared with the progress bar, they have no way to
  understand the difference.**

Corroborating product decisions:
- OpenAI, **2026-08-07**: *"Safeguards for background Voice conversations. On
  mobile, ChatGPT can recognize when a background Voice conversation no longer
  seems active, **check whether you're still there**, and end the session."* —
  they built an explicit "are you still there?" probe.
- OpenAI, **2026-08-21**: *"When you return to a conversation, completed answers
  now appear **without slowly replaying as though they're still being
  generated**."* — another false-active bug fixed.

### Guideline-set archaeology

Avouris & Sintoris (U. Patras), ACM EICS 2026, coded **51 principles** from five
guideline sets (1986–2019) by the human capacity they serve:

| Guideline set | Year | Cognitive | Communicational |
|---|---|---|---|
| Shneiderman's Golden Rules | 1986 | 38% | 18% |
| Nielsen's Usability Heuristics | 1994 | 46% | 32% |
| UXPA Principles | 2005 | 50% | 13% |
| ISO 9241-110 | 2006 | 57% | 9% |
| **Amershi et al. (Human–AI)** | **2019** | **23%** | **38%** |

Communication hits parity with cognition **for the first time**. Perception
scores 0–10% across all sets. Rater agreement: Nielsen α = **0.794** vs Human–AI
guidelines α = **0.532** — *"each of my rules cues one concern, whereas the AI
rules bundle several."* The authors state the corpus is small and validation thin;
treat percentages as directional.

### The canonical reference, still

- **Google PAIR Guidebook** (2019, updated 2021) — **23 design patterns** across
  7 questions, including *"Determine how to show model confidence, if at all"*
  and *"Give control back to the user when automation fails."*
- **Amershi et al. 2019**, 18 guidelines — the two most relevant to an always-on
  mic: **#3 Time services based on context** and **#8 Support efficient
  dismissal**.

---

## 5. What has failed

### 5a. Neural Expressive — a step back in usability

**Android Authority, 2026-06-18**, poll n=445: **49%** *"Yes, the colorful design
is just right"* / **13%** *"No, I don't like combined tools and attachments"* /
**13%** *"No, the sidebar is too crowded"* / 8% neutral / 12% other.

Specific failures:
- **Tools + attachments merged under one "+"** — *"Google forgot there's a
  difference between adding something to a prompt and completely changing what
  the prompt does."*
- **Account switcher moved out of reach** — a one-gesture action became two
  repeatable time-consuming gestures.
- Model selector relocated; Gems no longer quickly accessible.
- *"I shouldn't have to relearn how to use the Gemini app every couple of months."*

Earlier hands-on (2026-05-21, n=894): 44% yes / 15% love minimalist controls /
**14%** dislike merged tools / **15%** sidebar too crowded. *"There's no doubt
which one looks better — it's the fresh Neural Expressive app by a mile"* — but
*"the fresh appearance seems to come at the expense of utility."*

**TechCrunch, 2026-08-26** — Google's own promise is *"You shouldn't have to
guess whether a task requires Spark, a Daily Brief, or a quick inbox search."*
Reality: three separate branded features, each with its own icon and nav slot.
*"This clutters up what could otherwise be a more straightforward consumer
experience, and it suggests that Gemini is still struggling to find a killer
feature."*

**Support threads, 2026-05-19:** *"After the design update, **buttons no longer
fit on my phone screen**"*; *"they have **reduced the animation**."*

**A/B method worth copying:** Neural Expressive was a **server-side rollout**, so
one reviewer ran old and new side-by-side on two phones. *"That's the cheapest
way to validate an orb redesign."*

### 5b. Apple Liquid Glass — the most expensive documented redesign failure

| Date | Event |
|---|---|
| 2025-06-09 | Announced: *"our broadest design update ever."* |
| 2025-06-10 | WIRED: *"Beautiful and Hard to Read."* Allan Yu: *"mainly because I think they made it too transparent."* Josh Puckett: *"visually distracting… especially for users with visual impairments."* |
| 2025-06-10 | TechCrunch: Control Center *"almost unusable in the first developer beta."* White text on light wallpaper *"nearly fades away."* |
| 2025-06 | beta 2: darker blur + High Contrast border option |
| 2025-07-07 | beta 3: bolder tints. Reaction: *"iOS 26 beta 3 completely **nerfs** Liquid Glass. It looks so much **cheaper** now."* Reddit counter: *"it was pretty unreadable for anyone without perfect vision, and this addresses that, which is ultimately more important."* |
| 2025-10-20 | iOS 26.1: Clear/Tinted toggle |
| 2026-06-08 | WWDC26: rebuilt foundations for readability; new slider *"from ultra clear to fully tinted"* |

Long-tail complaints persist into 2026: *"There is currently **no true option to
fully disable Liquid Glass effects.**"*

> **Lesson:** a glass material over an unpredictable desktop background is exactly
> the unreadable case. Apple needed three betas, 12 months, and a user-facing
> slider to admit it. **Ship the opacity control in v1, or don't ship
> transparency.**

### 5c. Microsoft Copilot — brand proliferation, then retreat

- 2026-04-10: starts **removing** Copilot buttons from Notepad, Snipping Tool,
  Photos, Widgets — *"reducing unnecessary Copilot entry points."* Features stay;
  branding goes.
- 2026-09-10: Copilot+ PC branding retired entirely.

**Lesson:** users read branding as capability. Every feature gets its own icon and
nav slot and they stop trusting the whole.

### 5d. The Alexa+ shopping list — ambient verbosity destroying a simple task

The Verge, **2026-02-06**. Adding one item required: Whole Foods product images
(Amazon later admitted it was *"a short-term test"*), **two screens instead of
one**, a new Alexa chatbot text box at the bottom, and the Alexa Plus card pushing
Favorites and Devices behind it. Then:

> **User:** "Alexa, add sour cream to my shopping list."
> **Alexa+:** *"Looks like you're already stocked up on that creamy goodness! Sour
> cream is already chilling in your cart."*
> **User:** *"No. Thank you."*

Result: the reporter switched to Apple Reminders. **Apple Reminders: one tap.
Alexa: a diatribe.**

**The pattern:** *"a control surface got a chat surface grafted onto it."* A
power-user toggle became a persuasive narrator. **This is the exact failure mode
of putting a personality Lottie on an orb — the personality will get in the way
of the primary action.**

### 5e. Alexa+ capability failures (WIRED, 2026-03-06)

*"It claimed it was playing an episode of *The Pitt* when it was not — multiple
times."* Said the user *"wasn't seeing the show because it was paused."* Then
*"resumed" by replaying nature sounds from earlier in the day.*

**Lesson:** confidently wrong state reporting is worse than silence. An orb that
says "thinking" when it has crashed destroys more trust than an orb that visibly
gives up.

### 5f. Humane AI Pin + Rabbit R1

Covered in `10-competitive-audit-executive-summary.md` and earlier conversation. The transferable
line: **"The death was not caused by poor execution. It was caused by a thesis
that was wrong from the beginning"** — specifically, *"No task for which the AI
Pin was the best available tool."*

*"A failed hardware bet does not get a soft landing for its customers; it gets a
brick date."* — every Pin lost server connection 2025-02-28 12:00 PM Pacific.

Rabbit R1's UI, per Mashable: *"With the cute black-and-white rabbit icon
bouncing up and down the screen surrounded by a 'loud' color, the device reminded
me of my childhood obsession with '90s pocket toys like the Tamagotchi."*
**"Beautiful mess."**

### 5g. Google admitting reliability problems

**2026-07-08:** Gemini's Josh Woodward publicly asked *"what people were surprised
Gemini still couldn't do well."* **1,400+ replies in 12 hours, 1,700+ total.** Top
complaint: Workspace integration reliability. #2: inconsistent tool usage. This
came **right after** the Neural Expressive release.

### 5h. Anthropomorphic tone — a measured finding

Tamu Mays Business School, Gao & Sridhar, *Customer Needs and Solutions* 2026,
**n = 2,144** census-representative US adults, 120 items:

- **AI anxiety predicts MORE use, not less.** *"Grudge use — people keep working
  with a tool they resent because opting out feels costlier than the discomfort."*
- Usage shapes attitudes more than attitudes shape behaviour.
- Men more comfortable treating AI as a companion (4.1 vs 3.6); logged 1.7 h/day
  vs 1.2 h/day for women.
- Effect sizes hover near Cohen's *d* = 0.3 (small).

Design implication (Nielsen): *"Make anthropomorphism optional… Offer a plain
tool mode."*

⚠️ **This is self-reported attitudes, not measured responses to an animated
avatar. "This is not evidence that a talking orb fails."** Don't over-read it.

---

## 6. Privacy research — directly relevant to an always-on orb

- **>10% of recordings in one study were unintentional.** *"I have a friend also
  named Alexa who comes over, and Amazon Echo thinks we are giving it commands."*
  Users: *"There were times when the speaker would activate without me saying the
  wake word. This was a bit odd and it did leave me a bit uneasy."*
- **Users did not use available privacy controls.** Four participants said muting
  *"would negate the device's primary functionality."* Some believed they could
  mute by voice (*"Alexa, mute yourself"*). **Only one** participant ever deleted
  an audio log.
- Users wanted an **audible beep** when Google starts listening.
- Designers should *"make sure the app itself **switches colours** when it was in
  listening or watching mode."*
- One study found **~80% of users would enable short automatic recording
  deletion** if offered.
- **Silent recording is the worst case.** *"For some of these I remember hearing
  random responses back from Alexa but for the others, **I didn't notice the
  activation.**"* Users preferred a **smart-light notification** over a push
  notification because *"it works all the time, even if I'm not on a computer or
  phone."*

---

## 7. Recommended orb direction

Synthesising the above into a direction. **Not** "make it prettier" — a different
architecture for the state machine.

### Principles

1. **Collapsed by default.** Small and dim when idle, present when live. Every
   platform holder converged here.
2. **Geometry carries state, not personality.** Microsoft's removal of colour was
   deliberate. Use one accent, vary luminance and motion.
3. **Near-motionless at rest.** Apple: *"An ambient orb should be near-motionless
   by default."* And **avoid ~0.2 Hz oscillation** — our `breathe 3s` violates this.
4. **The collapsed state still shows state.** Apple: *"don't collapse to a bare
   logo… think about how your session can continue to convey information even in
   this tiny state."*
5. **Reserve red/amber strictly for errors.** Never for a normal state.
6. **Colour means aliveness, not identity.** Google's gradient encodes *"the model
   is working"*, not *which* state. Do not let hue mean "listening."
7. **Ship an opacity/dim control in v1** if translucent at all.
8. **Three independent axes, not one enum.** Following ChatGPT Dots:
   `activity` ⊥ `emotion` ⊥ `speech`. Our 4-state orb is the `activity` axis only.
   The fix for the three-states-share-one-segment bug is *orthogonality*, not
   three more animations.
9. **Zero focus stealing.** Wispr Flow's HUD *"does not steal input focus from the
   active text area."* Our non-activating sidebar pattern is the right instinct
   and should constrain the orb too.

### Explicitly not doing

- **Visemes / lip-sync.** Evidence says sync accuracy does not drive ratings
  (p = .62), synthesized beats captured (p < .001), audio-only ≈ audio+video, and
  desync is worse than nothing. If we ever animate the mouth, render it *ahead*
  of the audio and ship a user toggle.
- **Personality Lottie.** The Alexa+ sour-cream monologue.
- **A full-size animated orb.** Every 2026 platform holder moved away from it.
- **Throttling that breaks click-through.** See `01` §1.4c.

### Cheap wins available today

| Action | Cost | Source |
|---|---|---|
| Remove `breathe 3s` (0.33 Hz) | minutes | Apple HIG 0.2 Hz warning |
| Adopt M3 effects springs for colour/opacity | hours | `420/32` @150ms, `280/28` @200ms |
| Add a collapsed-by-default resting state | 1 day | Google, Microsoft, Apple all |
| Add an opacity/dim slider | hours | Liquid Glass lesson |
| Cap animation at 30fps | hours | perf |
| Deduplicate the `breathe` keyframes | minutes | `styles.css:99` and `setup.css:47` |

### How to validate

Copy Google's method: **server-side flag, old/new on two devices, same user.**
Neural Expressive was validated this way and it is the cheapest A/B available
for an orb redesign. Do not rebrand 4,704 lines of CSS against an unvalidated
direction.