# WhatsApp read-receipt check (do this BEFORE turning the watcher on)

NEXUS's WhatsApp watcher (`memcoreWhatsapp`, Memory page) ships **off** because one
thing cannot be known from code: whether the bridge's *read* calls (`list_chats`,
`list_messages`) cause WhatsApp to send a **read receipt (blue ticks)** or change
your **online / last-seen** status. The bridge's documentation is silent on it.
NEXUS itself never calls `mark_read`, `mark_chat_read`, `send_presence` or
`send_typing` (refused centrally in `mcp_client::call_tool`, with tests) - but the
bridge could still do something on its own when asked to list messages.

You need two phones (or a phone plus WhatsApp Web on a second account).

## Setup
1. Start the WhatsApp bridge (`127.0.0.1:8765`) and pair it with the phone that
   will be *watched* ("Phone A"). Use a spare/burner number if you can: unofficial
   WhatsApp clients can get a number banned.
2. In the Command Hub -> Memory, click **Check WhatsApp connection**. You want:
   bridge OK, N chats understood. (If it says it could not understand the chat
   list, stop - the parser needs the bridge's real format; send the note shown.)
3. Keep "Phone A" with **Read receipts ON** (Settings -> Privacy), otherwise the
   test proves nothing.
4. "Phone B" is the sender. Open the chat with Phone A on Phone B and keep the
   screen visible, with Phone A's name showing "last seen / online" if available.

## Test 1 - receipts
1. Turn on **Tell me when a priority person messages me on WhatsApp**.
2. From Phone B send "test 1" to Phone A. **Do not open the chat on Phone A.**
3. Wait for at least two poll cycles (about 2 minutes while you are at the PC).
4. Say "read that message" (or "read <Phone B's name>'s message") on the PC.
5. **Pass:** the message appears in the NEXUS sidebar, and on Phone B it still
   shows **two grey ticks** (delivered), never blue.
   **Fail:** blue ticks appear on Phone B without you opening the chat on Phone A.

## Test 2 - presence
1. On Phone B, look at Phone A's contact header ("online" / "last seen").
2. Make sure Phone A is idle and its WhatsApp is closed.
3. Let NEXUS poll for 5 minutes (leave the PC active, WhatsApp watcher on).
4. **Pass:** Phone A never shows "online" on Phone B because of NEXUS, and its
   last-seen does not change.
   **Fail:** "online" flickers or last-seen updates with each NEXUS poll.

## Test 3 - typing indicator
Phone B should never see "typing..." for Phone A at any time during tests 1-2.

## Result
- **All pass:** it is reasonable to keep the switch on. Note the bridge version you tested.
- **Any fail:** switch the watcher **off** and do not use it with this bridge. Tell
  me which test failed and what the bridge version is.
- A pass is evidence for *this bridge version on this account today*. WhatsApp
  changes behaviour server-side; repeat after bridge updates.

## What this does NOT cover
- If you reply from your **phone**, WhatsApp marks that chat read by itself.
  NEXUS cannot prevent or detect that.
- Account risk: WhatsApp's terms do not allow unofficial clients; bans happen.
- The bridge session must be re-paired roughly every 20 days.
