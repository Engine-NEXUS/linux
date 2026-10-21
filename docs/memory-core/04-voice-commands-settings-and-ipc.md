# 04 - Voice commands, settings and IPC

## Voice commands
| Say | Intent | Notes |
|---|---|---|
| "what do you remember" | memory_audit | grouped list with provenance |
| "remember that my dog is Bruno" / "forget my birthday" / "forget everything" (+ confirm) | memory_* | wipe rotates the key |
| "where did I leave off" / "show my briefing" | briefing | |
| "analyse this and add section 2 to my timetable" / "add the picture I copied to my timetable" | timetable_add | confirm before saving |
| "add those slots" / "show my timetable" / "clear my timetable" | timetable_commit/show/clear | |
| "use the browser for DSA" / "switch DSA to the app" | study_pref | overwritten on repeat |
| "any important emails" | mail_digest | |
| "that's not important" | mail_mute | mutes the LAST alert, mail or WhatsApp |
| "what's on my calendar today/tomorrow" | calendar_agenda | |
| "add dentist to my calendar tomorrow at 5pm" | calendar_add | read back + confirm |
| "read that message" / "read Asha's messages" / "what did Raj say on WhatsApp" | whatsapp_read | sidebar; spoken only if `memcoreWhatsappSpeak` |
| "make Asha a VIP" / "remove Raj from my VIPs" | people_flag (vip) | |
| "mute WhatsApp alerts from Raj" / "unmute WhatsApp alerts from Raj" | people_flag (mute) | needs "WhatsApp" + a name |
| "who are my priority people" | people_list | |

## Settings (settings.json, all must exist in `NexusSettings`)
| Key | Default | Meaning |
|---|---|---|
| `memcore` | on | master switch |
| `memcoreRedactNames` | off | Person A/B toward cloud models |
| `memcoreActivity` | on | record foreground window |
| `memcoreBriefing` | on | daily spoken briefing |
| `memcoreTimetable` | on | slot reminders |
| `memcoreMail` | on | inbox + calendar |
| `memcoreWhatsapp` | **off** | WhatsApp watcher |
| `memcoreWhatsappSpeak` | **off** | speak message text (goes to cloud voice) |

## IPC commands
`memcore_status`, `memcore_list`, `memcore_egress_log`, `memcore_clear_activity`,
`memcore_timetable`, `memcore_timetable_delete`, `memcore_mail_list`,
`memcore_mail_mute`, `memcore_people`, `memcore_person_flag`, `whatsapp_selftest`,
`offer_pending`, `google_credentials_status`, `google_save_custom_credentials`.

## Memory page (Command Hub -> Memory)
Summary + switches (redaction, activity, mail, WhatsApp, speak-aloud, timetable,
briefing), "Check WhatsApp connection", sections for important mail,
WhatsApp priority people (VIP/Mute), timetable, facts you told it, facts it
picked up, recent conversations, and the 7-day cloud-send log.
