# Mahayuddh UI Design Guidelines (Apple Music Web UI Specification)

> **Version:** 2.0.0  
> **Aesthetic:** Apple Music Web (music.apple.com) — High-contrast pitch black surfaces (#000000), frosted glass fixed sidebar navigation, Category Bento Tiles with rich color gradients, "Top Picks" hero banners, and a persistent "Now Solving" active session bottom bar.  
> **Target:** WCAG 2.2 AA Compliance  
> **Status:** Active Target Specification  

---

## 1. Context & Architectural Overview

The visual architecture is directly modeled after the **Apple Music Web App** layout (as captured in the reference UI):

`
+------------------------------------------------------------------------------------------+
¦   Mahayuddh    ¦ ?? Search problems, tags, friends...              [All] [Squad Sheets] ¦
¦                 +------------------------------------------------------------------------¦
¦  ?? Search      ¦                                                                        ¦
¦  ?? Arena       ¦  Top Picks for Squad (Hero Cards)                                      ¦
¦  ?? Leaderboard ¦  +--------------+ +--------------+ +--------------+ +--------------+  ¦
¦  ?? Suggestions ¦  ¦  Today POTD  ¦ ¦Streak Guardian¦ ¦Upcoming Contest¦ ¦ Top Suggestion ¦  ¦
¦                 ¦  +--------------+ +--------------+ +--------------+ +--------------+  ¦
¦  ROADMAPS       ¦                                                                        ¦
¦  ?? NeetCode150 ¦  Browse DSA Topics (Category Bento Grid)                               ¦
¦  ? Blind 75    ¦  +--------------+ +--------------+ +--------------+ +--------------+  ¦
¦  ?? Striver SDE ¦  ¦  Arrays/Hash ¦ ¦  DP Patterns ¦ ¦ Graphs & BFS ¦ ¦ Binary Search¦  ¦
¦                 ¦  +--------------+ +--------------+ +--------------+ +--------------+  ¦
¦  CUSTOM LISTS   ¦                                                                        ¦
¦  ?? Trees & BST ¦  Squad Activity Tracklist                                              ¦
¦  ?? 3AM Lore ?? ¦  #1  Prem Sai   ?? 14d   1920 Rating   [E:40 M:85 H:15]   [Suggest]    ¦
¦  ? New List    ¦  #2  Rahul K    ?? 8d    1810 Rating   [E:35 M:70 H:8]    [Suggest]    ¦
¦                 ¦                                                                        ¦
¦  ?? Prem Sai    ¦                                                                        ¦
+------------------------------------------------------------------------------------------¦
¦  [? ? ?] 00:24:15 ¦ ?? [Medium] 3Sum - Two Pointers (LeetCode #15) ¦ [Notes] [Push ??]  ¦
+------------------------------------------------------------------------------------------+
`

---

## 2. Design Tokens and Foundations

### Typography (Apple System Font Stack)
* **Primary Font:** ont.family.primary = -apple-system
* **Font Stack:** ont.family.stack = -apple-system, BlinkMacSystemFont, "SF Pro Display", "SF Pro Text", "Helvetica Neue", Helvetica, Arial, sans-serif
* **Base Size:** ont.size.base = 13px
* **Base Weight:** ont.weight.base = 400
* **Base Line Height:** ont.lineHeight.base = 16px

**Scale Hierarchy:**
* ont.size.xs = 10px *(Metadata, badges, sub-labels)*
* ont.size.sm = 11px *(Secondary captions, table column headers)*
* ont.size.md = 12px *(Problem difficulty tags, author names, timestamps)*
* ont.size.lg = 13px *(Base body, table cells, form inputs, sidebar items)*
* ont.size.xl = 14px *(Card titles, button labels, list headers)*
* ont.size.2xl = 16px *(Section headers, modal titles)*
* ont.size.3xl = 20px *(Metric numbers, streak counts, rating highlights)*
* ont.size.4xl = 28px *(Page titles: "Home", "Browse", "Leaderboard")*

### Color Palette (Apple Music Dark Theme)
* **Text:**
  * color.text.primary = #ffffff *(Main headings, card titles, high-contrast numbers)*
  * color.text.secondary = #8e8e93 *(Supporting metadata, problem slugs, timestamps)*
  * color.text.tertiary = #636366 *(Subtle icon fills, disabled hints)*
  * color.text.accent = #fa586a *(Apple Music signature Rose/Crimson — active sidebar, streak fire, primary CTAs)*
  * color.text.accentDark = #d60017 *(Deep crimson for high-contrast active backgrounds)*
* **Difficulty Badges:**
  * color.difficulty.easy = #30d158 *(Apple Green)*
  * color.difficulty.medium = #ff9f0a *(Apple Orange)*
  * color.difficulty.hard = #ff453a *(Apple Red)*
* **Surfaces & Layers:**
  * color.surface.base = #000000 *(Pure pitch-black background canvas)*
  * color.surface.sidebar = #121212 *(Sidebar background with subtle border right)*
  * color.surface.muted = #1c1c1e *(Card base backgrounds, search input fill)*
  * color.surface.raised = #2c2c2e *(Hovered sidebar links, table rows, active cards)*
  * color.surface.strong = #3a3a3c *(Modals, popovers, elevated controls)*
  * color.surface.bar = rgba(22, 22, 24, 0.85) *(Bottom bar with backdrop-filter blur 25px)*
* **Category Bento Gradients (Matching Screenshot Reference):**
  * gradient.crimson = linear-gradient(135deg, #e52d27 0%, #b31217 100%) *(Arrays & Hashing)*
  * gradient.sunset = linear-gradient(135deg, #ff9966 0%, #ff5e62 100%) *(Two Pointers & Sliding Window)*
  * gradient.royal = linear-gradient(135deg, #2b5876 0%, #4e4376 100%) *(Binary Search & Monotonic Stack)*
  * gradient.purple = linear-gradient(135deg, #8e2de2 0%, #4a00e0 100%) *(Dynamic Programming)*
  * gradient.emerald = linear-gradient(135deg, #11998e 0%, #38ef7d 100%) *(Trees & Tries)*
  * gradient.amber = linear-gradient(135deg, #f7971e 0%, #ffd200 100%) *(Graphs & Topological Sort)*
  * gradient.violet = linear-gradient(135deg, #654ea3 0%, #eaafc8 100%) *(Backtracking & Recursion)*
  * gradient.rose = linear-gradient(135deg, #fa709a 0%, #fee140 100%) *(Bit Manipulation & Math)*
* **Borders & Focus:**
  * color.border.default = rgba(255, 255, 255, 0.08) *(Subtle table and card dividers)*
  * color.border.strong = rgba(255, 255, 255, 0.16) *(Interactive borders, input focus rings)*
  * color.focus.ring = #fa586a *(High-contrast keyboard focus indicator with 2px offset)*

### Spacing Scale
* space.1 = 2px
* space.2 = 4px
* space.3 = 6px
* space.4 = 8px
* space.5 = 12px
* space.6 = 16px
* space.7 = 20px
* space.8 = 24px
* space.9 = 32px
* space.10 = 48px

### Radius & Elevation
* adius.xs = 4px *(Tags, inline code, micro badges)*
* adius.sm = 7px *(Inputs, sub-buttons, tooltips)*
* adius.md = 8px *(Standard buttons, alert cards)*
* adius.lg = 10px *(Table containers, problem cards)*
* adius.xl = 14px *(Category Bento cards, hero cards, dialogs)*
* adius.pill = 9999px *(Pill buttons, active filter chips, search bar)*
* shadow.subtle = 0 8px 24px rgba(0, 0, 0, 0.4)
* shadow.modal = 0 24px 64px rgba(0, 0, 0, 0.8)

---

## 3. Structural Component Rules

### 1. Left Fixed Sidebar Navigation
* **Dimensions:** Width 240px fixed, height 100vh, background color.surface.sidebar, right border 1px solid color.border.default.
* **Top Brand Area:** Apple Music style logo + App title (?? Mahayuddh), height 56px.
* **Search Item:** High-visibility pill button trigger (Search + Cmd+K), background color.surface.muted, text color.text.secondary, active/focused states highlight in color.text.accent (#fa586a).
* **Navigation Groups:**
  1. **Core:** Home (Arena), Leaderboards, Suggestion Box.
  2. **Library / Sheets:** NeetCode 150, Blind 75, Striver SDE Sheet.
  3. **Custom Playlists:** Group-created custom lists with emoji prefixes (?? Trees, ?? 3AM Lore ??, ? New List).
* **User Profile Footer:** Pinned to bottom of sidebar: User Avatar, Name, LeetCode handle, and Streak Flame badge (?? 14d).

### 2. Top Header & Search Bar
* **Anatomy:** Sticky header at top of main content (height: 56px, ackdrop-filter: blur(20px), g-surface-base/80).
* **Search Input:** Rounded pill (adius.pill), left search icon, placeholder "Search problems, tags, squad members...", keyboard shortcut badge ?K.
* **Right Controls:**
  * View Segment Toggle: [Squad] | [Global] pills.
  * Push Notification Status Bell (with badge).
  * Sync status indicator (e.g. Updated 2m ago).

### 3. Top Picks for Squad (Hero Cards Section)
* **Layout:** Horizontal grid (epeat(auto-fit, minmax(240px, 1fr))), height 220px.
* **Card Anatomy (Referencing image copy.png):**
  * Rounded corners adius.xl (14px).
  * Rich gradient or dark backdrop with geometric vector icon.
  * Top right badge: ?? Mahayuddh mini logo.
  * Bottom-left typography:
    * Category sub-label (ont.size.xs, uppercase, color.text.secondary, e.g. "PROBLEM OF THE DAY").
    * Main Card Title (ont.size.2xl, bold, color.text.primary, e.g. "LC #15: 3Sum").
    * Status / Squad Progress (ont.size.xs, e.g. "3/5 Friends Solved • 45 mins avg").

### 4. Category Bento Grid (Referencing image.png)
* **Layout:** 5-column responsive grid (grid-template-columns: repeat(auto-fill, minmax(210px, 1fr))), gap 16px.
* **Card Anatomy:**
  * Aspect ratio 16:9 or 4:3, adius.xl (14px), hover scale 1.02 with smooth transition (motion.fast = 200ms).
  * Deep rich gradient background (Crimson, Sunset, Purple, Emerald, Amber, Violet).
  * Bottom-left title (ont.size.xl, font weight 700, white text with subtle shadow).
  * Progress chip top-right (e.g., "18/25 Solved").

### 5. Tracklist-Style Squad Leaderboard
* **Row Anatomy:**
  * # Rank column (ont.size.md, color.text.secondary, Top 3 highlighted with Gold/Silver/Bronze crown).
  * Friend Avatar + Display Name + LeetCode link.
  * Streak flame badge (?? 14d in #fa586a).
  * Contest Rating with sparkline chart trend (1942 ?? +34).
  * Easy / Medium / Hard solved breakdown pills.
  * Action: "Suggest Problem" quick button.
* **Hover State:** Row background transitions to color.surface.raised with adius.md.

### 6. The "Now Solving" Bottom Bar (Active Session Player)
* **Anatomy (Referencing the player bar in image.png):**
  * Anchored to bottom of viewport, height 64px, color.surface.bar, ackdrop-filter: blur(25px), top border 1px solid color.border.default.
  * **Left:** Session Stopwatch / Timer ( 0:24:15) with Play / Pause / Reset controls.
  * **Center:** Current Problem Card:
    * Difficulty badge ([Medium]) + Problem Title (15. 3Sum) + Direct LeetCode Link.
    * Friend status indicator: *"Rahul & Prem are also solving this"*.
  * **Right:** Notes / Intuition Scratchpad toggle, Submit Check button, and Push Notifications toggle.

---

## 4. Mobile & PWA Responsive Behavior

* **Mobile Viewport (< 768px):**
  * Left sidebar collapses into an Apple Music style slide-out drawer via hamburger button.
  * Fixed bottom navigation tab bar (height: 54px, color.surface.bar) with 4 essential tabs:
    1. ?? **Arena** (Home & POTD)
    2. ?? **Leaderboard** (Rankings & Streaks)
    3. ?? **Topics** (Category Bento Grid)
    4. ?? **Inbox** (Suggestions & Nudges)
  * Touch targets are strictly **>= 44x44px** for all buttons and tabs.

---

## 5. Accessibility & QA Verification

* **Target:** WCAG 2.2 AA.
* **Keyboard Navigation:** Full focus trap inside modals; Arrow keys navigate tracklist rows; Cmd+K opens global search.
* **Contrast Requirements:**
  * #ffffff on #000000 -> **21:1** (AAA).
  * #8e8e93 on #1c1c1e -> **4.5:1** (AA).
  * #fa586a on #000000 -> **5.2:1** (AA).
* **Focus Visibility:** High-contrast 2px outline in #fa586a with 2px offset on all keyboard-focused elements.
