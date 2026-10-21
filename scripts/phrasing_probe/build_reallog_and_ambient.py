"""Builds probe_in2.jsonl: (a) every transcript in missed_intents.jsonl, (b) adversarial everyday sentences that
must NOT be accepted as commands. Usage: python build_reallog_and_ambient.py <outdir>"""
import json, os, sys
out = sys.argv[1]
rows = []
log = os.path.expandvars(r"%APPDATA%/com.nexus.assistant/missed_intents.jsonl")
for i, l in enumerate(x for x in open(log, encoding="utf-8") if x.strip()):
    rows.append(dict(id=f"reallog|{i}", action="reallog", mode="reallog", noise="", text=json.loads(l)["transcript"]))
AMBIENT = """I need to open a window to get some fresh air
let's go and grab a new tab at the restaurant
copy that over
can you close the door please
please send me the file tomorrow
there was a search party looking for him
i'll save you a seat
the back of the house is a mess
go back to what you were saying
what time does the next bus come
i can't find my keys anywhere
did you see the new episode last night
she said she would call me back later
we should refresh our memory about the plan
the screen is too bright in here
type of person who never listens
i want to play some music later
can you pass me the remote
he wrote a really long paper on this
stop talking and listen to me
you can just paste it into the document later
let me think about it for a second
i'll be right back
what's the weather like in london
open your mouth and say ah
my laptop is running really slow today
zoom is so annoying for meetings
click here to subscribe to the channel
the ghost in that movie was terrifying
turn left at the next light
we need to cancel our dinner reservation
she tabbed through all the options
select the best one for me
undo the damage that was done
it's going to be a long day
the first tab of the spreadsheet is blank
i think the browser is too old to run this
copy and paste is faster than typing
can you hear me now
the page is not loading
he closed the deal yesterday
my brother launched a startup last month
search engines have changed a lot
scroll through your phone less
send help
yes that sounds right
no i don't think so
okay then see you tomorrow
thank you for watching
that's what she said
play it again sam
go ahead and tell me what happened
everything is going to be fine
i'll go to the store later and get milk
have you saved any money this month
what is the capital of france
remind me to call mom
the new tab design looks better
close the window it's cold
okay google
hey siri what time is it
alexa play some jazz
nexus is a cool name
the mode of the data is five
I have a command for you""".split("\n")
for i, t in enumerate(AMBIENT):
    rows.append(dict(id=f"ambient|{i}", action="ambient", mode="ambient", noise="", text=t))
open(os.path.join(out, "probe_in2.jsonl"), "w", encoding="utf-8").write("\n".join(json.dumps(r) for r in rows) + "\n")
print(len(rows), "rows (", len(AMBIENT), "ambient )")
