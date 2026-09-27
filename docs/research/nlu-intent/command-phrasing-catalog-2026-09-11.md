# NEXUS Command Phrasing Catalog & Retraining Plan

**Date:** 2026-09-11
**Goal:** Catalog every possible phrasing for every NEXUS command, count them, and plan the BERT-Mini retraining with the new live-mode commands.

---

## Current State

| Metric | Value |
|--------|-------|
| Total intents in model | 47 |
| Training examples | 1,960 |
| Test examples | 397 |
| Examples per intent | 36–80 (avg ~42) |
| Live-mode intents in model | **0** (none trained) |
| New live-mode intents to add | **8** |

### Current Intent Distribution (from dataset.json)

```
80  list_prs              ← most examples
65  open_settings
52  comment_pr
51  list_releases
50  open_url
50  media_play_pause
50  create_release
50  greeting
49  delete_branch
49  analyse_latest_pr
48  list_branches
48  remove_collaborator
48  approve_pr
48  whatsapp_chat
48  close_app
48  list_workflow_runs
48  list_workflows
48  media_previous
47  list_collaborators
47  update_branch
47  list_pr_files
46  add_collaborator
46  list_org_members
46  media_next
46  open_app
45  merge_pr
45  cancel_workflow
45  remove_org_member
45  revert_pr
45  rerun_workflow
45  media_stop
44  add_org_member
44  check_branch
44  analyse_repo
44  close_pr
44  get_pr
44  open_architect
44  unknown
42  analyse_pr
42  search
36  create_pr              ← fewest examples
```

### Problem

The new live-mode commands (`type_text`, `press_key`, `press_hotkey`,
`confirm_send`, `cancel_action`, `browser_new_tab`, `browser_navigate`,
`browser_search`, `whatsapp_open`, `whatsapp_search`, `focus_app`) have
**zero training examples**. BERT-Mini will never classify them correctly
until we add training data.

---

## Complete Phrasing Catalog

For each command, I list every possible way a user might say it.
Categories:
- **Direct** — the most obvious phrasing
- **Conversational** — natural speech with filler words
- **STT mishearing** — what Whisper/Moonshine might transcribe
- **Shortened** — abbreviated or lazy speech
- **Polite** — with "please", "can you", etc.
- **Alternative verbs** — synonyms for the action verb
- **Negative examples** — phrases that should NOT match (for training the
  model to reject false positives)

### 1. open_app (current: 46 examples → target: 80)

**Slots:** `app_name`

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct verbs | open {app}, launch {app}, start {app}, run {app}, fire up {app}, bring up {app}, show {app}, pull up {app} | 8 verbs |
| Navigation verbs | go to {app}, visit {app}, browse to {app}, navigate to {app} | 4 verbs |
| With "the" | open the {app}, launch the {app}, start the {app}, show me the {app} | 4 |
| With "please" | open {app} please, please open {app}, can you open {app}, could you open {app} | 4 |
| With "for me" | open {app} for me, launch {app} for me, start {app} for me | 3 |
| With "app" suffix | open {app} app, launch the {app} application | 2 |
| Imperative casual | {app}, hey nexus open {app}, nexus open {app} | 3 |
| STT mishearings | open {app} (with phonetic variants of app name) | varies |
| Filler-prefixed | so open {app}, and open {app}, but first open {app}, let's open {app} | 4 |
| **Total verb×modifier combos** | | **~32 base patterns** |
| **With 20 app names × 32 patterns** | | **~640 possible phrasings** |

**Apps to use in training:** notepad, chrome, brave, firefox, vscode,
terminal, powershell, file explorer, calculator, spotify, whatsapp,
discord, slack, github desktop, obsidian, zoom, teams, outlook, word, excel

### 2. close_app (current: 48 → target: 70)

**Slots:** `app_name`

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct verbs | close {app}, quit {app}, exit {app}, kill {app}, terminate {app}, end {app}, shut down {app}, shut {app} | 8 verbs |
| With "the" | close the {app}, quit the {app}, kill the {app} | 3 |
| With "please" | close {app} please, please close {app}, can you close {app} | 3 |
| With "for me" | close {app} for me, quit {app} for me | 2 |
| Casual | shut {app}, bye {app}, get rid of {app} | 3 |
| STT mishearings | clothes {app} (close→clothes), quite {app} (quit→quite) | 2 |
| **Total base patterns** | | **~21** |
| **With 15 app names × 21 patterns** | | **~315 possible phrasings** |

### 3. open_url (current: 50 → target: 70)

**Slots:** `url`

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | open {url}, go to {url}, visit {url}, navigate to {url}, browse to {url} | 5 verbs |
| With https | open https://{url}, go to https://{url} | 2 |
| With www | open www.{url}, go to www.{url} | 2 |
| Bare URL | {url} (e.g. "github.com") | 1 |
| With "in browser" | open {url} in browser, open {url} in the browser | 2 |
| Known sites | open youtube, open gmail, open github, open google maps | 4 |
| With "website" | open {name} website, open {name} site, open {name} dot com | 3 |
| STT mishearings | open {name} dot com (when user says "{name}.com") | 1 |
| **Total base patterns** | | **~20** |
| **With 15 URLs × 20 patterns** | | **~300 possible phrasings** |

### 4. whatsapp_chat (current: 48 → target: 70)

**Slots:** `contact`

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | chat with {contact}, message {contact}, whatsapp {contact} | 3 |
| With "open" | open chat with {contact}, open my chat with {contact}, open whatsapp chat with {contact} | 3 |
| With "send" | send message to {contact}, send whatsapp to {contact}, send a message to {contact} | 3 |
| With "on" | chat with {contact} on whatsapp, message {contact} on whatsapp, whatsapp {contact} on wa | 3 |
| Casual | hit up {contact}, text {contact}, dm {contact}, drop a message to {contact} | 4 |
| With "please" | please chat with {contact}, can you message {contact} | 2 |
| STT mishearings | what's up {contact} (whatsapp→what's up), what sap {contact} | 2 |
| **Total base patterns** | | **~20** |
| **With 15 contacts × 20 patterns** | | **~300 possible phrasings** |

### 5. search (current: 42 → target: 65)

**Slots:** `query`

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct verbs | search for {query}, search {query}, google {query}, look up {query}, find {query}, find me {query}, look for {query} | 7 verbs |
| With "the" | search for the {query}, google the {query}, look up the {query} | 3 |
| With "please" | search for {query} please, can you google {query} | 2 |
| Casual | what is {query}, who is {query}, what's {query}, tell me about {query} | 4 |
| With "online" | search for {query} online, look up {query} on the internet | 2 |
| With "web" | search {query} on the web, find {query} on google | 2 |
| STT mishearings | search four {query} (for→four), look app {query} (up→app) | 2 |
| **Total base patterns** | | **~22** |
| **With 20 queries × 22 patterns** | | **~440 possible phrasings** |

### 6. greeting (current: 50 → target: 60)

**Slots:** none (or `greeting_type`)

| Category | Phrasings | Count |
|----------|-----------|-------|
| Hello | hello, hi, hey, yo, sup, hiya, hey nexus, hello nexus, hi nexus | 9 |
| How are you | how are you, how are you doing, how's it going, what's up, how do you do | 5 |
| Goodbye | bye, goodbye, see you, see you later, catch you later, later, bye nexus | 7 |
| Thanks | thanks, thank you, thanks nexus, thank you nexus, appreciate it, cheers, ty | 7 |
| Identity | who are you, what's your name, what are you, who is nexus, what can you do | 5 |
| Time of day | good morning, good afternoon, good evening, good night, morning, evening | 6 |
| Affirmative | yes, yeah, ok, okay, alright, sure, sounds good, understood, got it, will do | 10 |
| Negative | no, nope, nah, no thanks, never mind, forget it, cancel, disregard, stop | 9 |
| Welcome | you're welcome, no problem, no worries | 3 |
| **Total unique phrasings** | | **~61** |

### 7. media_play_pause (current: 50 → target: 60)

| Category | Phrasings | Count |
|----------|-----------|-------|
| Play | play, play music, play song, start playing, start music | 5 |
| Pause | pause, pause music, pause song, hit pause | 4 |
| Resume | resume, resume music, resume playback, continue playing, keep playing | 5 |
| Toggle | play pause, toggle media, toggle playback, play/pause | 4 |
| Casual | unpause, keep going, start the music, stop the music (ambiguous with stop) | 4 |
| Filler-prefixed | so play, so pause, so resume, and play music | 4 |
| **Total** | | **~26** |

### 8. media_next (current: 46 → target: 55)

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | next, next song, next track, skip, skip song, skip track | 6 |
| With "go" | go to next, go to the next song, go forward | 3 |
| Casual | move on, advance, forward, next one | 4 |
| **Total** | | **~13** |

### 9. media_previous (current: 48 → target: 55)

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | previous, previous song, previous track, prev, prev song | 5 |
| With "go" | go back, go to previous, go to the previous song, go back a song | 4 |
| Casual | last song, last track, rewind, back, previous one | 5 |
| **Total** | | **~14** |

### 10. media_stop (current: 45 → target: 50)

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | stop, stop music, stop playback, stop playing, stop the music | 5 |
| Casual | halt, cut the music, silence the music, kill the music | 4 |
| **Total** | | **~9** |

### 11. open_architect (current: 44 → target: 55)

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | open architecture mapper, open the architecture mapper, show architecture mapper | 3 |
| Variations | open architect, show architect, launch architect, start architect | 4 |
| With "codebase" | open codebase mapper, open dependency mapper, open codebase analyzer | 3 |
| With "map" | show architecture map, open architecture map, show the architecture map | 3 |
| Casual | architecture, mapper, open the mapper, show me the architecture | 4 |
| STT mishearings | open octach mapper, open arcade mapper, open ark mapper, open art mapper | 4 |
| **Total** | | **~21** |

### 12. open_settings (current: 65 → target: 65, already good)

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | open settings, show settings, open preferences, show preferences | 4 |
| With "command" | open command center, show command center, open the command center | 3 |
| With "config" | open config, open configuration, show config, configure nexus | 4 |
| Bare | settings, preferences, config, configuration | 4 |
| With "nexus" | nexus settings, nexus config, nexus preferences, nexus command center | 4 |
| Casual | let me configure, show me settings, take me to settings | 3 |
| **Total** | | **~22** |

---

## NEW Live-Mode Commands (need training data from scratch)

### 13. type_text (NEW — target: 60 examples)

**Slots:** `text`

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | type {text}, type in {text}, type out {text} | 3 |
| With "write" | write {text}, write out {text}, write in {text} | 3 |
| With "enter" | enter {text}, input {text}, insert {text} | 3 |
| With "put" | put {text}, put in {text}, put this in {text} | 3 |
| Casual | say {text}, type this: {text}, type that {text} | 3 |
| With "for me" | type {text} for me, write {text} for me | 2 |
| With "please" | please type {text}, can you type {text} | 2 |
| STT mishearings | typed {text}, typing {text}, right {text} (write→right) | 3 |
| Filler-prefixed | so type {text}, and type {text}, now type {text} | 3 |
| **Total base patterns** | | **~25** |
| **With 20 text samples × 25 patterns** | | **~500 possible phrasings** |

**Training text samples:** "hello world", "hi what are u doing", "hey mummy",
"good morning", "i'll be there in 5 minutes", "thanks a lot", "see you
tomorrow", "happy birthday", "call you back", "running late", "on my way",
"let's meet at 3pm", "the quick brown fox", "nexus is the best", "i love
coding", "what time is it", "how are you", "good night", "talk later",
"please review this"

### 14. press_key (NEW — target: 55 examples)

**Slots:** `key`

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | press {key}, hit {key}, tap {key}, strike {key} | 4 verbs |
| With "the" | press the {key}, hit the {key}, tap the {key} | 3 |
| Casual | {key} (just the key name), do {key}, send {key} | 3 |
| With "please" | press {key} please, can you press {key} | 2 |
| STT mishearings | pasture {key} (press→pasture), pressed {key}, pest {key} | 3 |
| Filler-prefixed | so press {key}, and press {key}, now press {key} | 3 |
| **Total base patterns** | | **~18** |
| **With 15 keys × 18 patterns** | | **~270 possible phrasings** |

**Training keys:** enter, escape, tab, space, backspace, delete, up, down,
left, right, home, end, f1, f5, f11

### 15. press_hotkey (NEW — target: 55 examples)

**Slots:** `keys` (Vec)

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | press {k1} {k2}, hit {k1} {k2}, press {k1} and {k2} | 3 |
| With "plus" | press {k1} plus {k2}, press {k1} + {k2} | 2 |
| With "with" | press {k1} with {k2}, {k1} with {k2} | 2 |
| Three-key | press {k1} {k2} {k3}, press {k1} shift {k3} | 2 |
| Casual | do {k1} {k2}, send {k1} {k2}, {k1} {k2} | 3 |
| STT mishearings | control {k2} (ctrl→control), alt {k2}, shift {k2} | 3 |
| **Total base patterns** | | **~15** |
| **With 10 combos × 15 patterns** | | **~150 possible phrasings** |

**Training combos:** ctrl+a, ctrl+c, ctrl+v, ctrl+x, ctrl+z, ctrl+y,
ctrl+s, ctrl+f, ctrl+t, ctrl+w, ctrl+tab, ctrl+shift+tab, alt+f4,
ctrl+shift+n, win+d

### 16. confirm_send (NEW — target: 30 examples)

**Slots:** none

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | send, send it, send message, send the message | 4 |
| With "now" | send now, send it now, go ahead and send | 3 |
| With "please" | please send, send please, go ahead please | 3 |
| Casual | ship it, fire it off, do it, confirm, yes send | 5 |
| With "enter" | hit enter, press enter to send, just send it | 3 |
| STT mishearings | sent, sand, send it (with emphasis) | 3 |
| **Total** | | **~21** |

### 17. cancel_action (NEW — target: 25 examples)

**Slots:** none

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | stop, cancel, cancel that, stop that | 4 |
| Casual | never mind, forget it, disregard, scratch that | 4 |
| With "don't" | don't send, don't do it, don't send it, wait don't | 4 |
| With "wait" | wait, hold on, wait a second, hold on a minute | 4 |
| STT mishearings | stoop (stop→stoop), cansel (cancel→cansel) | 2 |
| **Total** | | **~18** |

**Note:** "cancel" and "never mind" are currently caught by the greeting
parser. For live mode, the orchestrator should check if the state machine
has pending text and route to cancel_action instead of greeting.

### 18. browser_new_tab (NEW — target: 30 examples)

**Slots:** none

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | new tab, open new tab, open a new tab, new browser tab | 4 |
| With "please" | open a new tab please, can you open a new tab | 2 |
| Casual | tab, another tab, one more tab, start a new tab | 4 |
| With "window" | new window, open new window, open a new window | 3 |
| STT mishearings | new tap (tab→tap), open new tap | 2 |
| **Total** | | **~15** |

### 19. browser_navigate (NEW — target: 40 examples)

**Slots:** `url`

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | go to {url}, navigate to {url}, browse to {url} | 3 |
| With "open" | open {url} in this tab, open {url} here | 2 |
| With "visit" | visit {url}, take me to {url}, jump to {url} | 3 |
| Casual | {url}, head to {url}, point to {url} | 3 |
| **Total base patterns** | | **~11** |
| **With 15 URLs × 11 patterns** | | **~165 possible phrasings** |

### 20. browser_search (NEW — target: 40 examples)

**Slots:** `query`

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | search {query} in browser, search for {query} in browser | 2 |
| With "google" | google {query} in browser, google {query} here | 2 |
| With "this tab" | search {query} in this tab, look up {query} here | 2 |
| Casual | find {query} on the web, search {query} online | 2 |
| **Total base patterns** | | **~8** |
| **With 15 queries × 8 patterns** | | **~120 possible phrasings** |

### 21. whatsapp_open (NEW — target: 25 examples)

**Slots:** none

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | open whatsapp, launch whatsapp, start whatsapp | 3 |
| Casual | whatsapp, bring up whatsapp, show whatsapp | 3 |
| With "please" | open whatsapp please, can you open whatsapp | 2 |
| STT mishearings | open what's app, open what sap, open whats app | 3 |
| **Total** | | **~11** |

### 22. whatsapp_search (NEW — target: 35 examples)

**Slots:** `contact`

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | search for {contact} in whatsapp, find {contact} in whatsapp | 2 |
| With "chat" | open chat with {contact} in whatsapp, chat with {contact} on whatsapp | 2 |
| Casual | find {contact}, look for {contact}, search {contact} | 3 |
| With "go to" | go to {contact} chat, go to chat with {contact} | 2 |
| **Total base patterns** | | **~9** |
| **With 15 contacts × 9 patterns** | | **~135 possible phrasings** |

### 23. focus_app (NEW — target: 30 examples)

**Slots:** `target`

| Category | Phrasings | Count |
|----------|-----------|-------|
| Direct | focus {app}, bring {app} to front, bring {app} forward | 3 |
| With "switch" | switch to {app}, switch over to {app}, switch to {app} window | 3 |
| Casual | go to {app}, bring up {app}, show {app}, maximize {app} | 4 |
| With "window" | focus {app} window, bring {app} window to front | 2 |
| **Total base patterns** | | **~12** |
| **With 10 apps × 12 patterns** | | **~120 possible phrasings** |

---

## Existing Commands (GitHub + Analysis — already well-trained)

### 24-47. GitHub Commands (keep current counts)

These are already well-represented (42-80 examples each). No changes
needed except adding a few STT mishearing variants:

| Intent | Current | Target | New phrasings to add |
|--------|---------|--------|---------------------|
| merge_pr | 45 | 50 | +5 STT mishearings |
| approve_pr | 48 | 50 | +2 |
| close_pr | 44 | 48 | +4 |
| list_prs | 80 | 80 | already good |
| get_pr | 44 | 48 | +4 |
| create_pr | 36 | 45 | +9 (needs more) |
| update_branch | 47 | 50 | +3 |
| revert_pr | 45 | 48 | +3 |
| list_pr_files | 47 | 50 | +3 |
| comment_pr | 52 | 52 | already good |
| add_collaborator | 46 | 50 | +4 |
| remove_collaborator | 48 | 50 | +2 |
| list_collaborators | 47 | 50 | +3 |
| add_org_member | 44 | 48 | +4 |
| remove_org_member | 45 | 48 | +3 |
| list_org_members | 46 | 48 | +2 |
| delete_branch | 49 | 50 | +1 |
| list_branches | 48 | 50 | +2 |
| create_release | 50 | 50 | already good |
| list_releases | 51 | 51 | already good |
| list_workflows | 48 | 50 | +2 |
| list_workflow_runs | 48 | 50 | +2 |
| rerun_workflow | 45 | 48 | +3 |
| cancel_workflow | 45 | 48 | +3 |

### 48-51. Analysis Commands (keep current counts)

| Intent | Current | Target | New phrasings to add |
|--------|---------|--------|---------------------|
| analyse_repo | 44 | 50 | +6 (STT mishearings) |
| analyse_pr | 42 | 48 | +6 |
| analyse_latest_pr | 49 | 50 | +1 |
| check_branch | 44 | 48 | +4 |

---

## Summary: New Intent Count

| Intent | Current Examples | Target Examples | New to Generate |
|--------|-----------------|----------------|-----------------|
| **type_text** | 0 | 60 | **60** |
| **press_key** | 0 | 55 | **55** |
| **press_hotkey** | 0 | 55 | **55** |
| **confirm_send** | 0 | 30 | **30** |
| **cancel_action** | 0 | 25 | **25** |
| **browser_new_tab** | 0 | 30 | **30** |
| **browser_navigate** | 0 | 40 | **40** |
| **browser_search** | 0 | 40 | **40** |
| **whatsapp_open** | 0 | 25 | **25** |
| **whatsapp_search** | 0 | 35 | **35** |
| **focus_app** | 0 | 30 | **30** |
| open_app | 46 | 80 | +34 |
| close_app | 48 | 70 | +22 |
| open_url | 50 | 70 | +20 |
| whatsapp_chat | 48 | 70 | +22 |
| search | 42 | 65 | +23 |
| greeting | 50 | 60 | +10 |
| media_play_pause | 50 | 60 | +10 |
| media_next | 46 | 55 | +9 |
| media_previous | 48 | 55 | +7 |
| media_stop | 45 | 50 | +5 |
| open_architect | 44 | 55 | +11 |
| open_settings | 65 | 65 | 0 |
| analyse_repo | 44 | 50 | +6 |
| analyse_pr | 42 | 48 | +6 |
| analyse_latest_pr | 49 | 50 | +1 |
| check_branch | 44 | 48 | +4 |
| create_pr | 36 | 45 | +9 |
| Other GitHub (24 intents) | ~46 avg | ~50 | +~60 total |
| **TOTAL NEW** | | | **~620 new examples** |

**New dataset size:** 1,960 + 620 = **~2,580 training examples**
**New intent count:** 47 + 11 = **58 intents**

---

## Negative Examples (Critical for Preventing False Positives)

The user specifically requested negative learning — the model must
DISCARD wrong possibilities, not just add new ones.

### Commands that sound similar and must be distinguished:

| Confusion | Correct intent | Wrong intent | Example |
|-----------|---------------|-------------|---------|
| "type" vs "open" | type_text: "type hello" | open_app: "type hello" | "type" is NOT an open verb |
| "press" vs "open" | press_key: "press enter" | open_app: "press enter" | "press" is NOT an open verb |
| "send" vs "search" | confirm_send: "send" | search: "send" | "send" alone is NOT a search |
| "stop" vs media_stop | cancel_action: "stop" | media_stop: "stop music" | "stop" alone = cancel, "stop music" = media |
| "new tab" vs "open" | browser_new_tab: "new tab" | open_app: "new tab" | "new tab" is NOT an app |
| "go to" vs "open" | browser_navigate: "go to github.com" | open_app: "go to github" | URL → navigate, app name → open |
| "chat with" vs "open" | whatsapp_chat: "chat with mom" | open_app: "chat with mom" | "chat with" is NOT an open verb |
| "search" vs "google" | search: "search for cats" | open_url: "search for cats" | "search for" → search intent |
| "cancel" vs "close" | greeting/cancel: "cancel" | close_app: "cancel" | "cancel" alone is NOT close |

### Negative training examples to add (target: 100):

```json
{"text": "type notepad", "intent": "type_text", "slots": {"text": "notepad"}}
// NOT open_app — "type" is a live-mode verb, not an open verb

{"text": "press chrome", "intent": "press_key", "slots": {"key": "chrome"}}
// NOT open_app — "press" is a live-mode verb

{"text": "send cats", "intent": "confirm_send", "slots": {}}
// NOT search — "send" alone is confirm, not search

{"text": "stop", "intent": "cancel_action", "slots": {}}
// NOT media_stop — "stop" alone is cancel, "stop music" is media_stop

{"text": "new tab", "intent": "browser_new_tab", "slots": {}}
// NOT open_app — "new tab" is a browser action
```

---

## Retraining Plan

### Step 1: Generate New Training Data

Create a Python script that generates the new examples programmatically:

```python
# server/nlu/generate_live_data.py

APPS = ["notepad", "chrome", "brave", "vscode", "terminal", ...]
KEYS = ["enter", "escape", "tab", "space", "backspace", ...]
HOTKEYS = [["ctrl", "a"], ["ctrl", "c"], ["ctrl", "v"], ...]
TEXTS = ["hello world", "hi what are u doing", "hey mummy", ...]
CONTACTS = ["mom", "dad", "lakshya", "john", "mummy", ...]
URLS = ["github.com", "google.com", "youtube.com", ...]

# Generate type_text examples
for text in TEXTS:
    for pattern in ["type {text}", "write {text}", "enter {text}", ...]:
        examples.append({"text": pattern.format(text=text), "intent": "type_text", ...})

# Generate press_key examples
for key in KEYS:
    for pattern in ["press {key}", "hit {key}", "tap {key}", ...]:
        examples.append({"text": pattern.format(key=key), "intent": "press_key", ...})
```

### Step 2: Update INTENTS list in train.py

Add the 11 new intents:

```python
INTENTS = [
    # ... existing 47 intents ...
    # New live-mode intents:
    "type_text",         # slots: text
    "press_key",         # slots: key
    "press_hotkey",      # slots: keys
    "confirm_send",      # no slots
    "cancel_action",     # no slots
    "browser_new_tab",   # no slots
    "browser_navigate",  # slots: url
    "browser_search",    # slots: query
    "whatsapp_open",     # no slots
    "whatsapp_search",   # slots: contact
    "focus_app",         # slots: target
]
```

### Step 3: Update SLOT_TYPES in train.py

Add new slot types:

```python
SLOT_TYPES = [
    # ... existing slots ...
    "text",        # for type_text
    "key",         # for press_key
    "keys",        # for press_hotkey
    "url",         # for browser_navigate (may already exist)
    "query",       # for browser_search (may already exist)
    "contact",     # for whatsapp_search (may already exist)
    "target",      # for focus_app
]
```

### Step 4: Merge with Existing Data

Use the existing `merge_and_train.py` to merge new examples with the
existing dataset:

```bash
cd server/nlu
python generate_live_data.py --output new_examples.json
python merge_and_train.py --new-data new_examples.json
python train.py
```

### Step 5: Verify Model Quality

After training, test with:

```bash
# Test live-mode commands
python -c "
from nlu_server import classify
tests = [
    'type hello world',
    'press enter',
    'press ctrl a',
    'send it',
    'stop',
    'new tab',
    'open whatsapp',
    'focus chrome',
]
for t in tests:
    result = classify(t)
    print(f'{t:30s} → {result[\"intent\"]:20s} (conf: {result[\"confidence\"]:.2f})')
"
```

### Step 6: Continuous Learning Loop

The admin Qwen brain should:
1. Watch for misclassifications (when the deterministic parser overrides
   the NLU result)
2. Log the correct intent + the misheard text
3. Add to the training dataset
4. Retrain weekly (or when 50+ new examples accumulate)

### Step 7: Negative Learning

The brain should also:
1. Watch for false positives (NLU returns an intent but the user corrects
   it or the action fails)
2. Log the text + the WRONG intent + the CORRECT intent
3. Add as a negative example: same text, correct intent
4. Retrain to suppress the wrong classification

---

## Phrasing Count Summary

| Command | Possible Phrasings | Target Training Examples |
|---------|-------------------|------------------------|
| open_app | ~640 | 80 |
| close_app | ~315 | 70 |
| open_url | ~300 | 70 |
| whatsapp_chat | ~300 | 70 |
| search | ~440 | 65 |
| greeting | ~61 | 60 |
| media_play_pause | ~26 | 60 |
| media_next | ~13 | 55 |
| media_previous | ~14 | 55 |
| media_stop | ~9 | 50 |
| open_architect | ~21 | 55 |
| open_settings | ~22 | 65 |
| **type_text** | **~500** | **60** |
| **press_key** | **~270** | **55** |
| **press_hotkey** | **~150** | **55** |
| **confirm_send** | **~21** | **30** |
| **cancel_action** | **~18** | **25** |
| **browser_new_tab** | **~15** | **30** |
| **browser_navigate** | **~165** | **40** |
| **browser_search** | **~120** | **40** |
| **whatsapp_open** | **~11** | **25** |
| **whatsapp_search** | **~135** | **35** |
| **focus_app** | **~120** | **30** |
| GitHub (24 intents) | ~100 each | ~50 each |
| Analysis (4 intents) | ~50 each | ~50 each |
| **TOTAL** | **~3,500+** | **~2,580** |

The model can't learn all 3,500+ possible phrasings, but with ~2,580
well-distributed examples across 58 intents, it should generalize to
recognize the patterns and handle unseen variations.
