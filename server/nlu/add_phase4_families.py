#!/usr/bin/env python3
"""
Phase 4 — Expand training data for weak intents and OOS rejection.

Authors structurally diverse training examples for the 13 intents that
lost coverage during Phase 1 quarantine, plus additional `unknown` examples
targeting the false-positive patterns discovered in Phase 3.

All new examples use command structures that do NOT share phrase families
with the frozen final test. This is verified after generation.

Output: server/nlu/data/phase4_expanded_families.json
"""
import json
import re
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
OUTPUT_PATH = SCRIPT_DIR / "data" / "phase4_expanded_families.json"

FILLERS = {"please", "could", "would", "you", "kindly", "can"}


def normalize(text):
    return " ".join(str(text).lower().strip().split())


def family_text(row):
    text = normalize(row.get("text", ""))
    values = []
    for slot, value in sorted((row.get("slots") or {}).items()):
        slot_values = value if isinstance(value, list) else [value]
        for item in slot_values:
            item_text = normalize(item)
            if item_text:
                values.append((len(item_text), item_text, f"<{slot}>"))
    for _, value, replacement in sorted(values, reverse=True):
        text = text.replace(value, replacement)
    tokens = re.findall(r"<[a-z_]+>|[a-z]+|\d+", text)
    while tokens and tokens[0] in FILLERS:
        tokens.pop(0)
    tokens = ["<number>" if token.isdigit() else token for token in tokens]
    return " ".join(tokens)


def family_key(row):
    return f"{row.get('intent', '')}|{family_text(row)}"


def build_examples():
    examples = []

    # ─── approve_pr ──────────────────────────────────────────────
    # Test families: "approve pull request N in R", "approve pr N in R",
    #                "approve the pr N in R"
    # New families must use different structures.
    approve_templates = [
        ("mark pull request 42 in {repo} as approved", {"repo": "octo/tools", "pr_number": "42"}),
        ("give approval for pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("sign off on pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("greenlight pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("accept the review for pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("lgtm pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("ok the pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("approve the changes in pr 42 for {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("mark pr 42 in {repo} as good to merge", {"repo": "octo/tools", "pr_number": "42"}),
        ("give the thumbs up to pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("sign off pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("approve review on pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("accept pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("greenlight pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("mark pull request 42 in {repo} approved", {"repo": "octo/tools", "pr_number": "42"}),
        ("give pr 42 in {repo} the go ahead", {"repo": "octo/tools", "pr_number": "42"}),
        ("approve the code review for pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("sign off on the review for pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("lgtm pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("ok pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("accept the pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("approve review for pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("mark the pull request 42 in {repo} as approved", {"repo": "octo/tools", "pr_number": "42"}),
        ("give a thumbs up to pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("sign off the pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("approve the code changes in pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("give your approval to pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("mark pr 42 in {repo} as good", {"repo": "octo/tools", "pr_number": "42"}),
        ("accept the changes in pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("ok the review for pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
    ]
    for text, slots in approve_templates:
        examples.append({"text": text, "intent": "approve_pr", "slots": slots})

    # ─── close_pr ────────────────────────────────────────────────
    # Test families: "close pr N in R", "close pull request N in R",
    #                "shut down pr N in R", "close the pr N in R"
    close_templates = [
        ("dismiss pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("reject pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("shut pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("close out pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("dismiss pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("reject pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("shut pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("close out pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("terminate pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("cancel pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("dismiss the pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("reject the pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("close down pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("shut down pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("terminate pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("cancel pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("dismiss the pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("reject the pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("close down pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("terminate the pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("cancel the pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("close the pull request 42 in {repo} without merging", {"repo": "octo/tools", "pr_number": "42"}),
        ("dismiss pr 42 in {repo} as not needed", {"repo": "octo/tools", "pr_number": "42"}),
        ("reject pr 42 in {repo} as invalid", {"repo": "octo/tools", "pr_number": "42"}),
        ("shut down the pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("close pull request 42 in {repo} without merge", {"repo": "octo/tools", "pr_number": "42"}),
        ("terminate the pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("cancel the pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("close out the pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("dismiss the pull request 42 in {repo} as resolved", {"repo": "octo/tools", "pr_number": "42"}),
    ]
    for text, slots in close_templates:
        examples.append({"text": text, "intent": "close_pr", "slots": slots})

    # ─── comment_pr ──────────────────────────────────────────────
    # Test families: "comment pr N in R: body", "comment on pr N in R: body"
    comment_templates = [
        ("leave a comment on pull request 42 in {repo} saying good job", {"repo": "octo/tools", "pr_number": "42", "body": "good job"}),
        ("write a review comment on pr 42 in {repo} saying needs tests", {"repo": "octo/tools", "pr_number": "42", "body": "needs tests"}),
        ("add a review note to pull request 42 in {repo} saying looks fine", {"repo": "octo/tools", "pr_number": "42", "body": "looks fine"}),
        ("post a comment on pr 42 in {repo} saying ship it", {"repo": "octo/tools", "pr_number": "42", "body": "ship it"}),
        ("leave a note on pull request 42 in {repo} saying fix the lint", {"repo": "octo/tools", "pr_number": "42", "body": "fix the lint"}),
        ("write a comment for pull request 42 in {repo} saying approved", {"repo": "octo/tools", "pr_number": "42", "body": "approved"}),
        ("add a comment to pr 42 in {repo} saying rebase needed", {"repo": "octo/tools", "pr_number": "42", "body": "rebase needed"}),
        ("post a review on pull request 42 in {repo} saying good work", {"repo": "octo/tools", "pr_number": "42", "body": "good work"}),
        ("leave feedback on pr 42 in {repo} saying minor changes", {"repo": "octo/tools", "pr_number": "42", "body": "minor changes"}),
        ("write a note on pull request 42 in {repo} saying lgtm", {"repo": "octo/tools", "pr_number": "42", "body": "lgtm"}),
        ("add review feedback to pr 42 in {repo} saying looks solid", {"repo": "octo/tools", "pr_number": "42", "body": "looks solid"}),
        ("post feedback on pull request 42 in {repo} saying needs docs", {"repo": "octo/tools", "pr_number": "42", "body": "needs docs"}),
        ("leave a review on pr 42 in {repo} saying good to go", {"repo": "octo/tools", "pr_number": "42", "body": "good to go"}),
        ("write feedback for pull request 42 in {repo} saying merge after ci", {"repo": "octo/tools", "pr_number": "42", "body": "merge after ci"}),
        ("add a note to pull request 42 in {repo} saying update the readme", {"repo": "octo/tools", "pr_number": "42", "body": "update the readme"}),
        ("post a note on pr 42 in {repo} saying looks great", {"repo": "octo/tools", "pr_number": "42", "body": "looks great"}),
        ("leave a review comment for pull request 42 in {repo} saying nit pick", {"repo": "octo/tools", "pr_number": "42", "body": "nit pick"}),
        ("write a review note for pr 42 in {repo} saying question here", {"repo": "octo/tools", "pr_number": "42", "body": "question here"}),
        ("add review comments to pull request 42 in {repo} saying suggest refactor", {"repo": "octo/tools", "pr_number": "42", "body": "suggest refactor"}),
        ("post a review note on pr 42 in {repo} saying blocking issue", {"repo": "octo/tools", "pr_number": "42", "body": "blocking issue"}),
        ("leave a remark on pull request 42 in {repo} saying nice cleanup", {"repo": "octo/tools", "pr_number": "42", "body": "nice cleanup"}),
        ("write a remark for pr 42 in {repo} saying consider renaming", {"repo": "octo/tools", "pr_number": "42", "body": "consider renaming"}),
        ("add a remark to pull request 42 in {repo} saying test coverage low", {"repo": "octo/tools", "pr_number": "42", "body": "test coverage low"}),
        ("post a remark on pr 42 in {repo} saying good approach", {"repo": "octo/tools", "pr_number": "42", "body": "good approach"}),
        ("leave a review remark for pull request 42 in {repo} saying edge case missing", {"repo": "octo/tools", "pr_number": "42", "body": "edge case missing"}),
    ]
    for text, slots in comment_templates:
        examples.append({"text": text, "intent": "comment_pr", "slots": slots})

    # ─── add_org_member ──────────────────────────────────────────
    # Test families: "invite U to org O", "add U as ROLE to org O", "add U to org O"
    add_org_templates = [
        ("bring sarah into the {org} organization", {"org": "devhub", "username": "sarah"}),
        ("add mike to the {org} organization as a member", {"org": "devhub", "username": "mike", "role": "member"}),
        ("invite laura to join the {org} organization", {"org": "devhub", "username": "laura"}),
        ("welcome tom as a new member of org {org}", {"org": "devhub", "username": "tom", "role": "member"}),
        ("bring jenny into org {org} with admin access", {"org": "devhub", "username": "jenny", "role": "admin"}),
        ("add frank to the {org} organization with member role", {"org": "devhub", "username": "frank", "role": "member"}),
        ("invite grace to become a member of org {org}", {"org": "devhub", "username": "grace", "role": "member"}),
        ("welcome alex into the {org} organization as an admin", {"org": "devhub", "username": "alex", "role": "admin"}),
        ("bring nina to org {org} as a member", {"org": "devhub", "username": "nina", "role": "member"}),
        ("add owen to the {org} org with push access", {"org": "devhub", "username": "owen", "role": "admin"}),
        ("invite rita to the {org} organization as admin", {"org": "devhub", "username": "rita", "role": "admin"}),
        ("welcome sam to org {org} as a regular member", {"org": "devhub", "username": "sam", "role": "member"}),
        ("bring tina into the {org} organization with member privileges", {"org": "devhub", "username": "tina", "role": "member"}),
        ("add victor to org {org} and give him admin rights", {"org": "devhub", "username": "victor", "role": "admin"}),
        ("invite wendy to join org {org} as a member", {"org": "devhub", "username": "wendy", "role": "member"}),
        ("welcome yuki into org {org} with admin privileges", {"org": "devhub", "username": "yuki", "role": "admin"}),
        ("bring zara to the {org} organization as a new member", {"org": "devhub", "username": "zara", "role": "member"}),
        ("add charlie to the {org} organization and grant admin", {"org": "devhub", "username": "charlie", "role": "admin"}),
        ("invite dana to org {org} with member access", {"org": "devhub", "username": "dana", "role": "member"}),
        ("welcome evan into the {org} organization as admin", {"org": "devhub", "username": "evan", "role": "admin"}),
        ("bring fiona to org {org} and make her a member", {"org": "devhub", "username": "fiona", "role": "member"}),
        ("add george to the {org} org as an administrator", {"org": "devhub", "username": "george", "role": "admin"}),
        ("invite helen to become an admin of org {org}", {"org": "devhub", "username": "helen", "role": "admin"}),
        ("welcome ivan to the {org} organization with member status", {"org": "devhub", "username": "ivan", "role": "member"}),
        ("bring julia into org {org} as an admin member", {"org": "devhub", "username": "julia", "role": "admin"}),
        ("add kevin to the {org} organization granting member access", {"org": "devhub", "username": "kevin", "role": "member"}),
        ("invite lily to the {org} org and assign admin role", {"org": "devhub", "username": "lily", "role": "admin"}),
        ("welcome mark into the {org} organization as a new admin", {"org": "devhub", "username": "mark", "role": "admin"}),
        ("bring nora to the {org} organization with admin rights", {"org": "devhub", "username": "nora", "role": "admin"}),
        ("add oscar to org {org} as a regular member", {"org": "devhub", "username": "oscar", "role": "member"}),
    ]
    for text, slots in add_org_templates:
        examples.append({"text": text, "intent": "add_org_member", "slots": slots})

    # ─── remove_org_member ───────────────────────────────────────
    # Test families: "remove U from organization O", "kick U from org O",
    #                "delete U from org O"
    remove_org_templates = [
        ("drop sarah from the {org} organization", {"org": "devhub", "username": "sarah"}),
        ("take mike out of org {org}", {"org": "devhub", "username": "mike"}),
        ("revoke laura from the {org} organization", {"org": "devhub", "username": "laura"}),
        ("remove tom from the {org} org roster", {"org": "devhub", "username": "tom"}),
        ("drop jenny from org {org}", {"org": "devhub", "username": "jenny"}),
        ("take frank out of the {org} organization", {"org": "devhub", "username": "frank"}),
        ("revoke grace from org {org}", {"org": "devhub", "username": "grace"}),
        ("remove alex from the {org} organization membership", {"org": "devhub", "username": "alex"}),
        ("drop nina from the {org} org", {"org": "devhub", "username": "nina"}),
        ("take owen out of the {org} organization roster", {"org": "devhub", "username": "owen"}),
        ("revoke rita from the {org} org membership", {"org": "devhub", "username": "rita"}),
        ("remove sam from org {org} membership", {"org": "devhub", "username": "sam"}),
        ("drop tina from the {org} organization member list", {"org": "devhub", "username": "tina"}),
        ("take victor out of the {org} org", {"org": "devhub", "username": "victor"}),
        ("revoke wendy from the {org} organization", {"org": "devhub", "username": "wendy"}),
        ("remove yuki from the {org} org member list", {"org": "devhub", "username": "yuki"}),
        ("drop zara from org {org} membership", {"org": "devhub", "username": "zara"}),
        ("take charlie out of the {org} organization", {"org": "devhub", "username": "charlie"}),
        ("revoke dana from the {org} org", {"org": "devhub", "username": "dana"}),
        ("remove evan from org {org}", {"org": "devhub", "username": "evan"}),
        ("drop fiona from the {org} organization roster", {"org": "devhub", "username": "fiona"}),
        ("take george out of the {org} org membership", {"org": "devhub", "username": "george"}),
        ("revoke helen from the {org} organization member list", {"org": "devhub", "username": "helen"}),
        ("remove ivan from the {org} org roster", {"org": "devhub", "username": "ivan"}),
        ("drop julia from org {org} member list", {"org": "devhub", "username": "julia"}),
        ("take kevin out of the {org} organization membership", {"org": "devhub", "username": "kevin"}),
        ("revoke lily from the {org} org roster", {"org": "devhub", "username": "lily"}),
        ("remove mark from the {org} organization", {"org": "devhub", "username": "mark"}),
        ("drop nora from the {org} org membership list", {"org": "devhub", "username": "nora"}),
        ("take oscar out of org {org}", {"org": "devhub", "username": "oscar"}),
    ]
    for text, slots in remove_org_templates:
        examples.append({"text": text, "intent": "remove_org_member", "slots": slots})

    # ─── revert_pr ───────────────────────────────────────────────
    # Test families: "revert pr N in R", "rollback pr N in R",
    #                "revert pull request N in R", "revert the pr N in R"
    revert_templates = [
        ("undo the changes from pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("roll back pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("reverse pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("undo pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("revert the changes in pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("roll back the pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("reverse pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("undo the pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("revert the merge of pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("roll back the changes from pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("reverse the pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("undo the pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("revert the commit from pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("roll back pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("reverse the changes from pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("undo the merge of pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("revert the pull request 42 merge in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("roll back the merge of pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("reverse the merge of pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("undo the commit from pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("revert changes from pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("roll back changes from pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("reverse changes from pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("undo changes from pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("revert the code from pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("roll back the code from pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("reverse the code from pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("undo the code from pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("revert what pull request 42 did in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("roll back what pr 42 changed in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
    ]
    for text, slots in revert_templates:
        examples.append({"text": text, "intent": "revert_pr", "slots": slots})

    # ─── merge_pr ────────────────────────────────────────────────
    # Test families: "squash merge pr N in R", "merge pull request N in R",
    #                "merge pr N in R", "rebase merge pr N in R"
    merge_templates = [
        ("combine pull request 42 into {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("integrate pr 42 into {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("merge the pull request 42 into {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("combine pr 42 into {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("integrate pull request 42 into {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("merge the pr 42 into {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("combine the pull request 42 into {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("integrate the pr 42 into {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("merge pull request 42 into the {repo} repository", {"repo": "octo/tools", "pr_number": "42"}),
        ("combine pr 42 into the {repo} repository", {"repo": "octo/tools", "pr_number": "42"}),
        ("integrate pull request 42 into the {repo} repository", {"repo": "octo/tools", "pr_number": "42"}),
        ("merge the pull request 42 into the {repo} repository", {"repo": "octo/tools", "pr_number": "42"}),
        ("combine the pr 42 into the {repo} repository", {"repo": "octo/tools", "pr_number": "42"}),
        ("integrate the pull request 42 into the {repo} repository", {"repo": "octo/tools", "pr_number": "42"}),
        ("merge pr 42 into the {repo} repository", {"repo": "octo/tools", "pr_number": "42"}),
        ("combine the pull request 42 into the {repo} repo", {"repo": "octo/tools", "pr_number": "42"}),
        ("integrate pr 42 into the {repo} repo", {"repo": "octo/tools", "pr_number": "42"}),
        ("merge the pr 42 into the {repo} repo", {"repo": "octo/tools", "pr_number": "42"}),
        ("combine pull request 42 into the {repo} repo", {"repo": "octo/tools", "pr_number": "42"}),
        ("integrate the pull request 42 into the {repo} repo", {"repo": "octo/tools", "pr_number": "42"}),
        ("merge pull request 42 into {repo} with a merge commit", {"repo": "octo/tools", "pr_number": "42"}),
        ("combine pr 42 into {repo} using squash", {"repo": "octo/tools", "pr_number": "42"}),
        ("integrate pull request 42 into {repo} with rebase", {"repo": "octo/tools", "pr_number": "42"}),
        ("merge the pr 42 into {repo} using a squash merge", {"repo": "octo/tools", "pr_number": "42"}),
        ("combine the pull request 42 into {repo} with a rebase merge", {"repo": "octo/tools", "pr_number": "42"}),
        ("integrate the pr 42 into {repo} with a merge commit", {"repo": "octo/tools", "pr_number": "42"}),
        ("merge pull request 42 into {repo} main branch", {"repo": "octo/tools", "pr_number": "42"}),
        ("combine pr 42 into {repo} default branch", {"repo": "octo/tools", "pr_number": "42"}),
        ("integrate pull request 42 into {repo} develop branch", {"repo": "octo/tools", "pr_number": "42"}),
        ("merge the pr 42 into {repo} master branch", {"repo": "octo/tools", "pr_number": "42"}),
    ]
    for text, slots in merge_templates:
        examples.append({"text": text, "intent": "merge_pr", "slots": slots})

    # ─── cancel_workflow ─────────────────────────────────────────
    # Test families: "abort workflow N in R", "stop workflow N in R",
    #                "halt workflow N in R"
    cancel_wf_templates = [
        ("terminate workflow 789 in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("cancel workflow 456 in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("kill workflow 123 in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("end workflow 789 in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("terminate the workflow 456 in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("cancel the workflow 123 in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("kill the workflow 789 in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("end the workflow 456 in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("terminate workflow run 123 in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("cancel workflow run 789 in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("kill workflow run 456 in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("end workflow run 123 in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("terminate the workflow run 789 in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("cancel the workflow run 456 in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("kill the workflow run 123 in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("end the workflow run 789 in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("stop the workflow 456 in {repo} immediately", {"repo": "octo/tools", "workflow_id": "456"}),
        ("abort the workflow 123 in {repo} now", {"repo": "octo/tools", "workflow_id": "123"}),
        ("halt the workflow 789 in {repo} right away", {"repo": "octo/tools", "workflow_id": "789"}),
        ("terminate workflow 456 in {repo} immediately", {"repo": "octo/tools", "workflow_id": "456"}),
        ("cancel workflow 123 in {repo} now", {"repo": "octo/tools", "workflow_id": "123"}),
        ("kill workflow 789 in {repo} right away", {"repo": "octo/tools", "workflow_id": "789"}),
        ("end workflow 456 in {repo} as soon as possible", {"repo": "octo/tools", "workflow_id": "456"}),
        ("terminate the workflow run 123 in {repo} now", {"repo": "octo/tools", "workflow_id": "123"}),
        ("cancel the workflow run 789 in {repo} immediately", {"repo": "octo/tools", "workflow_id": "789"}),
        ("kill the workflow run 456 in {repo} right now", {"repo": "octo/tools", "workflow_id": "456"}),
        ("end the workflow run 123 in {repo} right away", {"repo": "octo/tools", "workflow_id": "123"}),
        ("stop workflow 789 in {repo} and discard changes", {"repo": "octo/tools", "workflow_id": "789"}),
        ("abort workflow 456 in {repo} and clean up", {"repo": "octo/tools", "workflow_id": "456"}),
        ("halt workflow 123 in {repo} and remove artifacts", {"repo": "octo/tools", "workflow_id": "123"}),
    ]
    for text, slots in cancel_wf_templates:
        examples.append({"text": text, "intent": "cancel_workflow", "slots": slots})

    # ─── add_collaborator ────────────────────────────────────────
    # Test families: "add U as ROLE collaborator to R"
    add_collab_templates = [
        ("grant sarah push access to {repo}", {"repo": "octo/tools", "username": "sarah", "permission": "push"}),
        ("give mike write access to {repo}", {"repo": "octo/tools", "username": "mike", "permission": "push"}),
        ("add laura as a collaborator on {repo} with pull access", {"repo": "octo/tools", "username": "laura", "permission": "pull"}),
        ("grant tom admin access to {repo}", {"repo": "octo/tools", "username": "tom", "permission": "admin"}),
        ("give jenny triage access to {repo}", {"repo": "octo/tools", "username": "jenny", "permission": "triage"}),
        ("add frank as a collaborator to {repo} with maintain access", {"repo": "octo/tools", "username": "frank", "permission": "maintain"}),
        ("grant grace push permissions on {repo}", {"repo": "octo/tools", "username": "grace", "permission": "push"}),
        ("give alex write permissions on {repo}", {"repo": "octo/tools", "username": "alex", "permission": "push"}),
        ("add nina as a collaborator on {repo} with admin access", {"repo": "octo/tools", "username": "nina", "permission": "admin"}),
        ("grant owen pull permissions on {repo}", {"repo": "octo/tools", "username": "owen", "permission": "pull"}),
        ("give rita maintain access on {repo}", {"repo": "octo/tools", "username": "rita", "permission": "maintain"}),
        ("add sam as a collaborator to {repo} with triage access", {"repo": "octo/tools", "username": "sam", "permission": "triage"}),
        ("grant tina admin permissions on {repo}", {"repo": "octo/tools", "username": "tina", "permission": "admin"}),
        ("give victor push access on {repo}", {"repo": "octo/tools", "username": "victor", "permission": "push"}),
        ("add wendy as a collaborator to {repo} with write access", {"repo": "octo/tools", "username": "wendy", "permission": "push"}),
        ("grant yuki pull access on {repo}", {"repo": "octo/tools", "username": "yuki", "permission": "pull"}),
        ("give zara maintain permissions on {repo}", {"repo": "octo/tools", "username": "zara", "permission": "maintain"}),
        ("add charlie as a collaborator on {repo} with push access", {"repo": "octo/tools", "username": "charlie", "permission": "push"}),
        ("grant dana triage access on {repo}", {"repo": "octo/tools", "username": "dana", "permission": "triage"}),
        ("give evan admin access on {repo}", {"repo": "octo/tools", "username": "evan", "permission": "admin"}),
        ("add fiona as a collaborator to {repo} with pull permissions", {"repo": "octo/tools", "username": "fiona", "permission": "pull"}),
        ("grant george write access on {repo}", {"repo": "octo/tools", "username": "george", "permission": "push"}),
        ("give helen maintain access on {repo}", {"repo": "octo/tools", "username": "helen", "permission": "maintain"}),
        ("add ivan as a collaborator on {repo} with admin permissions", {"repo": "octo/tools", "username": "ivan", "permission": "admin"}),
        ("grant julia push access on {repo}", {"repo": "octo/tools", "username": "julia", "permission": "push"}),
        ("give kevin triage permissions on {repo}", {"repo": "octo/tools", "username": "kevin", "permission": "triage"}),
        ("add lily as a collaborator to {repo} with maintain permissions", {"repo": "octo/tools", "username": "lily", "permission": "maintain"}),
        ("grant mark pull access on {repo}", {"repo": "octo/tools", "username": "mark", "permission": "pull"}),
        ("give nora write permissions on {repo}", {"repo": "octo/tools", "username": "nora", "permission": "push"}),
        ("add oscar as a collaborator on {repo} with triage permissions", {"repo": "octo/tools", "username": "oscar", "permission": "triage"}),
    ]
    for text, slots in add_collab_templates:
        examples.append({"text": text, "intent": "add_collaborator", "slots": slots})

    # ─── create_release ──────────────────────────────────────────
    # Test families: "create release TAG in R", "publish release TAG in R",
    #                "make a release TAG for R"
    create_rel_templates = [
        ("cut a new release v1.2.3 for {repo}", {"repo": "octo/tools", "release_tag": "v1.2.3"}),
        ("tag a release v2.0.0 in {repo}", {"repo": "octo/tools", "release_tag": "v2.0.0"}),
        ("publish version v1.5.0 as a release in {repo}", {"repo": "octo/tools", "release_tag": "v1.5.0"}),
        ("create a github release for v3.0.0 in {repo}", {"repo": "octo/tools", "release_tag": "v3.0.0"}),
        ("cut release v1.0.0 for {repo}", {"repo": "octo/tools", "release_tag": "v1.0.0"}),
        ("tag version v2.1.0 as a release in {repo}", {"repo": "octo/tools", "release_tag": "v2.1.0"}),
        ("publish the release v1.4.0 for {repo}", {"repo": "octo/tools", "release_tag": "v1.4.0"}),
        ("create a new release for v2.2.0 in {repo}", {"repo": "octo/tools", "release_tag": "v2.2.0"}),
        ("cut a release for v1.3.0 in {repo}", {"repo": "octo/tools", "release_tag": "v1.3.0"}),
        ("tag a new release v3.1.0 for {repo}", {"repo": "octo/tools", "release_tag": "v3.1.0"}),
        ("publish version v1.6.0 as a github release in {repo}", {"repo": "octo/tools", "release_tag": "v1.6.0"}),
        ("create the release for v2.3.0 in {repo}", {"repo": "octo/tools", "release_tag": "v2.3.0"}),
        ("cut a github release v1.1.0 for {repo}", {"repo": "octo/tools", "release_tag": "v1.1.0"}),
        ("tag the release v3.2.0 in {repo}", {"repo": "octo/tools", "release_tag": "v3.2.0"}),
        ("publish a new release v1.7.0 for {repo}", {"repo": "octo/tools", "release_tag": "v1.7.0"}),
        ("create release v2.4.0 for the {repo} repository", {"repo": "octo/tools", "release_tag": "v2.4.0"}),
        ("cut version v1.8.0 as a release for {repo}", {"repo": "octo/tools", "release_tag": "v1.8.0"}),
        ("tag a github release v3.3.0 in {repo}", {"repo": "octo/tools", "release_tag": "v3.3.0"}),
        ("publish the version v1.9.0 as a release in {repo}", {"repo": "octo/tools", "release_tag": "v1.9.0"}),
        ("create a release tagged v2.5.0 for {repo}", {"repo": "octo/tools", "release_tag": "v2.5.0"}),
        ("cut a new version v1.0.5 as a release in {repo}", {"repo": "octo/tools", "release_tag": "v1.0.5"}),
        ("tag the version v2.6.0 as a release for {repo}", {"repo": "octo/tools", "release_tag": "v2.6.0"}),
        ("publish a github release for v3.4.0 in {repo}", {"repo": "octo/tools", "release_tag": "v3.4.0"}),
        ("create the github release v1.2.0 for {repo}", {"repo": "octo/tools", "release_tag": "v1.2.0"}),
        ("cut the release v2.7.0 for {repo}", {"repo": "octo/tools", "release_tag": "v2.7.0"}),
        ("tag a new version v1.3.5 as a release in {repo}", {"repo": "octo/tools", "release_tag": "v1.3.5"}),
        ("publish the github release v3.5.0 for {repo}", {"repo": "octo/tools", "release_tag": "v3.5.0"}),
        ("create a new github release for v1.4.5 in {repo}", {"repo": "octo/tools", "release_tag": "v1.4.5"}),
        ("cut a release tagged v2.8.0 in {repo}", {"repo": "octo/tools", "release_tag": "v2.8.0"}),
        ("tag the github release v1.5.5 for {repo}", {"repo": "octo/tools", "release_tag": "v1.5.5"}),
    ]
    for text, slots in create_rel_templates:
        examples.append({"text": text, "intent": "create_release", "slots": slots})

    # ─── delete_branch ────────────────────────────────────────────
    # Test families: "delete branch B in R", "remove branch B in R",
    #                "delete the branch B in R"
    delete_br_templates = [
        ("drop the feature-x branch from {repo}", {"repo": "octo/tools", "branch": "feature-x"}),
        ("get rid of the hotfix-1 branch in {repo}", {"repo": "octo/tools", "branch": "hotfix-1"}),
        ("eliminate the dev branch from {repo}", {"repo": "octo/tools", "branch": "dev"}),
        ("remove the bugfix-2 branch from {repo}", {"repo": "octo/tools", "branch": "bugfix-2"}),
        ("drop branch feature-y from {repo}", {"repo": "octo/tools", "branch": "feature-y"}),
        ("get rid of branch hotfix-3 in {repo}", {"repo": "octo/tools", "branch": "hotfix-3"}),
        ("eliminate branch dev-branch from {repo}", {"repo": "octo/tools", "branch": "dev-branch"}),
        ("remove branch test-branch from {repo}", {"repo": "octo/tools", "branch": "test-branch"}),
        ("drop the old-branch from {repo}", {"repo": "octo/tools", "branch": "old-branch"}),
        ("get rid of the stale-branch in {repo}", {"repo": "octo/tools", "branch": "stale-branch"}),
        ("eliminate the temp-branch from {repo}", {"repo": "octo/tools", "branch": "temp-branch"}),
        ("remove the wip-branch from {repo}", {"repo": "octo/tools", "branch": "wip-branch"}),
        ("drop branch cleanup from {repo}", {"repo": "octo/tools", "branch": "cleanup"}),
        ("get rid of branch experiment in {repo}", {"repo": "octo/tools", "branch": "experiment"}),
        ("eliminate branch refactor from {repo}", {"repo": "octo/tools", "branch": "refactor"}),
        ("remove branch migration from {repo}", {"repo": "octo/tools", "branch": "migration"}),
        ("drop the legacy-branch from {repo}", {"repo": "octo/tools", "branch": "legacy-branch"}),
        ("get rid of the deprecated-branch in {repo}", {"repo": "octo/tools", "branch": "deprecated-branch"}),
        ("eliminate the unused-branch from {repo}", {"repo": "octo/tools", "branch": "unused-branch"}),
        ("remove the merged-branch from {repo}", {"repo": "octo/tools", "branch": "merged-branch"}),
        ("drop branch old-feature from {repo}", {"repo": "octo/tools", "branch": "old-feature"}),
        ("get rid of branch old-hotfix in {repo}", {"repo": "octo/tools", "branch": "old-hotfix"}),
        ("eliminate branch old-dev from {repo}", {"repo": "octo/tools", "branch": "old-dev"}),
        ("remove branch old-test from {repo}", {"repo": "octo/tools", "branch": "old-test"}),
        ("drop the branch feature-abc from {repo}", {"repo": "octo/tools", "branch": "feature-abc"}),
        ("get rid of the branch hotfix-xyz in {repo}", {"repo": "octo/tools", "branch": "hotfix-xyz"}),
        ("eliminate the branch dev-123 from {repo}", {"repo": "octo/tools", "branch": "dev-123"}),
        ("remove the branch test-456 from {repo}", {"repo": "octo/tools", "branch": "test-456"}),
        ("drop branch release-candidate from {repo}", {"repo": "octo/tools", "branch": "release-candidate"}),
        ("get rid of branch patch-1 in {repo}", {"repo": "octo/tools", "branch": "patch-1"}),
    ]
    for text, slots in delete_br_templates:
        examples.append({"text": text, "intent": "delete_branch", "slots": slots})

    # ─── get_pr ──────────────────────────────────────────────────
    # Test families: "get pr N in R", "show pr N in R", "view pr N in R",
    #                "open pr N in R", "fetch pr N in R"
    get_pr_templates = [
        ("pull up pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("display pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("show me the pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("fetch the pr 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("pull up pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("display the pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("show me pr 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("fetch pull request 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("pull up the pull request 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("display pr 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("show me the pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("fetch the pull request 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("pull up the pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("display the pr 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("show me pull request 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("fetch pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("pull up pull request 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("display the pull request 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("show me the pr 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("fetch the pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("bring up pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("look at pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("bring up pr 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("look at pull request 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("bring up the pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("look at the pr 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("bring up the pr 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("look at the pull request 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("open up pull request 42 in {repo}", {"repo": "octo/tools", "pr_number": "42"}),
        ("check out pr 42 from {repo}", {"repo": "octo/tools", "pr_number": "42"}),
    ]
    for text, slots in get_pr_templates:
        examples.append({"text": text, "intent": "get_pr", "slots": slots})

    # ─── rerun_workflow ──────────────────────────────────────────
    # Test families: "rerun workflow N in R", "restart workflow N in R",
    #                "retry workflow N in R"
    rerun_wf_templates = [
        ("run workflow 789 again in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("re-execute workflow 456 in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("trigger workflow 123 again in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("run the workflow 789 again in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("re-execute the workflow 456 in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("trigger the workflow 123 again in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("run workflow 789 once more in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("re-execute workflow 456 once more in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("trigger workflow 123 once more in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("run the workflow 789 once more in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("re-execute the workflow 456 once more in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("trigger the workflow 123 once more in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("run workflow 789 from the start in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("re-execute workflow 456 from scratch in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("trigger workflow 123 from the beginning in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("run the workflow 789 from the start in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("re-execute the workflow 456 from scratch in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("trigger the workflow 123 from the beginning in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("run workflow 789 a second time in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("re-execute workflow 456 a second time in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("trigger workflow 123 a second time in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("run the workflow 789 a second time in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("re-execute the workflow 456 a second time in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("trigger the workflow 123 a second time in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("run workflow 789 one more time in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("re-execute workflow 456 one more time in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("trigger workflow 123 one more time in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
        ("run the workflow 789 one more time in {repo}", {"repo": "octo/tools", "workflow_id": "789"}),
        ("re-execute the workflow 456 one more time in {repo}", {"repo": "octo/tools", "workflow_id": "456"}),
        ("trigger the workflow 123 one more time in {repo}", {"repo": "octo/tools", "workflow_id": "123"}),
    ]
    for text, slots in rerun_wf_templates:
        examples.append({"text": text, "intent": "rerun_workflow", "slots": slots})

    # ─── Additional OOS examples targeting false-positive patterns ─
    # Phase 3 showed type_text, focus_app, browser_new_tab are prone to
    # false activation on words like "put", "text", "new", "save", "show".
    oos_templates = [
        # type_text false positives (put, words, text, paper, write)
        "put into words what you want",
        "put words on the paper about what you want",
        "what do i put on my feet",
        "do i have to put my mouth on theirs when doing cpr",
        "where should i put my keys",
        "put the book on the shelf",
        "put the milk in the fridge",
        "what words rhyme with orange",
        "how many words are in a page",
        "write a poem about the ocean",
        "write a letter to my grandmother",
        "write down my dreams from last night",
        "put together a grocery list for the week",
        "what should i write in a birthday card",
        "words that start with the letter q",
        "how do you spell accommodation",
        "put the leftovers in the container",
        "write a thank you note for the gift",
        "put the puzzle pieces together",
        "words with double letters in them",
        # focus_app false positives (show, game, save, laptop)
        "show me a cool nintendo switch game",
        "save my text on my laptop hard drive",
        "show me pictures of cute puppies",
        "save the document to my desktop",
        "show me how to tie a tie",
        "save my progress in the game",
        "show me the weather forecast",
        "save the file to my documents folder",
        "show me a map of downtown",
        "save the screenshot to my pictures",
        "show me the latest news headlines",
        "save the email to my drafts folder",
        "show me a recipe for chocolate cake",
        "save the webpage as a bookmark",
        "show me the score of the game",
        "save my notes from the meeting",
        "show me directions to the airport",
        "save the receipt for my taxes",
        "show me the time in tokyo",
        "save the presentation to my drive",
        # browser_new_tab false positives (new, install, program)
        "program my new robot to bring me snacks",
        "i want to install new tiles in my kitchen",
        "i need a new pair of shoes",
        "how do i program my thermostat",
        "where can i buy a new phone case",
        "i want to learn a new language",
        "program the garage door opener",
        "new restaurants near my house",
        "i need to install a new battery",
        "how do i program my tv remote",
        "new movies coming out this weekend",
        "i want to try a new recipe",
        "program the sprinkler system",
        "new books by my favorite author",
        "i need to install new software",
        "how do i program a calculator",
        "new games for the playstation",
        "i want to buy a new car",
        "program the coffee maker",
        "new songs on the radio",
        # general unsupported
        "report outage to my electric provider",
        "how much is an overdraft fee for bank",
        "what size wipers does this car take",
        "where is the dipstick",
        "how much is 1 share of aapl",
        "what time does the louvre open",
        "how many sides are in a hexagon",
        "why are exponents performed before multiplication",
        "what is the capital of mongolia",
        "how does a microwave work",
    ]
    for text in oos_templates:
        examples.append({"text": text, "intent": "unknown", "slots": {}})

    return examples


def verify_no_test_family_overlap(examples, test_rows):
    test_families = {family_key(r) for r in test_rows}
    new_families = set()
    for ex in examples:
        fk = family_key(ex)
        if fk in test_families:
            raise ValueError(f"new example shares family with frozen test: {ex['text']} -> {fk}")
        new_families.add(fk)
    return len(new_families)


def main():
    examples = build_examples()

    # Load frozen test to verify no family overlap
    with open(SCRIPT_DIR / "dataset.json", "r", encoding="utf-8") as f:
        dataset = json.load(f)
    test_rows = dataset["test"]

    unique_families = verify_no_test_family_overlap(examples, test_rows)

    from collections import Counter
    intent_counts = Counter(e["intent"] for e in examples)

    output = {
        "schema_version": 1,
        "source": "nexus_synthetic_phase4",
        "review_status": "approved_for_candidate_training",
        "policy": "Structurally diverse command forms that do not share phrase families with the frozen final test. Additional OOS examples target false-positive patterns discovered in Phase 3.",
        "total_examples": len(examples),
        "unique_families": unique_families,
        "intent_counts": dict(intent_counts.most_common()),
        "examples": examples,
    }

    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(OUTPUT_PATH, "w", encoding="utf-8") as f:
        json.dump(output, f, indent=2, ensure_ascii=False)

    print(f"Phase 4 expanded families: {len(examples)} examples, {unique_families} unique families")
    print(f"Intent counts:")
    for intent, count in intent_counts.most_common():
        print(f"  {intent}: {count}")
    print(f"Output: {OUTPUT_PATH}")
    print("No phrase family overlap with frozen test: verified")


if __name__ == "__main__":
    main()
