#!/usr/bin/env python3
"""
Unit tests for COSMIC custom shortcut registration and RON parsing.
Validates the logic used in scripts/register-hotkey.sh.
"""

import os
import re
import tempfile
import unittest


def parse_entries(content):
    content = content.strip()
    if not content:
        return []
    start = content.find('{')
    end = content.rfind('}')
    if start == -1 or end == -1:
        return []
    inner = content[start + 1:end]
    entries = []
    i = 0
    n = len(inner)
    while i < n:
        while i < n and inner[i] in ' \t\r\n,':
            i += 1
        if i >= n:
            break
        if inner[i] == '(':
            entry_start = i
            paren_depth = 0
            in_str = False
            escape = False
            while i < n:
                c = inner[i]
                if escape:
                    escape = False
                elif c == '\\':
                    escape = True
                elif c == '"':
                    in_str = not in_str
                elif not in_str:
                    if c == '(':
                        paren_depth += 1
                    elif c == ')':
                        paren_depth -= 1
                        if paren_depth == 0:
                            i += 1
                            break
                i += 1
            while i < n and inner[i] in ' \t\r\n':
                i += 1
            if i < n and inner[i] == ':':
                i += 1
            in_str = False
            escape = False
            val_paren_depth = 0
            while i < n:
                c = inner[i]
                if escape:
                    escape = False
                elif c == '\\':
                    escape = True
                elif c == '"':
                    in_str = not in_str
                elif not in_str:
                    if c == '(':
                        val_paren_depth += 1
                    elif c == ')':
                        val_paren_depth -= 1
                    elif c == ',' and val_paren_depth == 0:
                        i += 1
                        break
                    elif c == '}' and val_paren_depth == 0:
                        break
                i += 1
            entry_text = inner[entry_start:i].strip().rstrip(',')
            if entry_text:
                entries.append(entry_text)
        else:
            i += 1
    return entries


def is_nexus_entry(entry):
    if re.search(r'description:\s*Some\(\s*"[^"]*nexus[^"]*"\s*\)', entry, re.I):
        return True
    if re.search(r'Spawn\(\s*"[^"]*nexus[^"]*--wake[^"]*"\s*\)', entry, re.I):
        return True
    return False


def update_cosmic_shortcuts(file_path, nexus_bin, mods, key):
    if not mods:
        raise ValueError("Cannot bind key with no modifiers")

    kept_entries = []
    if os.path.exists(file_path):
        with open(file_path, "r", encoding="utf-8") as f:
            raw = f.read()
        entries = parse_entries(raw)
        for e in entries:
            if not is_nexus_entry(e):
                kept_entries.append(e)

    mods_formatted = "\n".join(f"            {m}," for m in mods)
    new_nexus_entry = f"""    (
        modifiers: [
{mods_formatted}
        ],
        key: "{key}",
        description: Some("Nexus"),
    ): Spawn("{nexus_bin} --wake"),"""

    lines = ["{"]
    for e in kept_entries:
        indented = "\n".join("    " + line if not line.startswith("    ") else line for line in e.splitlines())
        lines.append(indented + ",")
    lines.append(new_nexus_entry)
    lines.append("}")
    output = "\n".join(lines) + "\n"

    with open(file_path, "w", encoding="utf-8") as f:
        f.write(output)


class TestCosmicShortcuts(unittest.TestCase):
    def test_parse_entries(self):
        sample = """{
    (
        modifiers: [
            Super,
        ],
        key: "t",
        description: Some("Terminal"),
    ): Spawn("cosmic-term"),
    (
        modifiers: [
            Ctrl,
            Super,
        ],
        key: "space",
        description: Some("Nexus"),
    ): Spawn("/usr/bin/nexus --wake"),
}"""
        entries = parse_entries(sample)
        self.assertEqual(len(entries), 2)
        self.assertFalse(is_nexus_entry(entries[0]))
        self.assertTrue(is_nexus_entry(entries[1]))

    def test_update_replaces_nexus_and_keeps_other(self):
        with tempfile.NamedTemporaryFile("w+", delete=False) as tf:
            tf.write("""{
    (
        modifiers: [
            Super,
        ],
        key: "t",
        description: Some("Terminal"),
    ): Spawn("cosmic-term"),
    (
        modifiers: [],
        key: "bracketright",
        description: Some("Nexus"),
    ): Spawn("/old/debug/nexus --wake"),
}""")
            tf_path = tf.name

        try:
            update_cosmic_shortcuts(tf_path, "/new/release/nexus", ["Ctrl", "Super"], "space")
            with open(tf_path, "r") as f:
                content = f.read()

            entries = parse_entries(content)
            self.assertEqual(len(entries), 2)
            # Terminal preserved
            self.assertIn("cosmic-term", entries[0])
            # Old nexus removed, new nexus added
            self.assertNotIn("/old/debug/nexus", content)
            self.assertIn("/new/release/nexus --wake", content)
            self.assertIn('description: Some("Nexus")', content)
        finally:
            if os.path.exists(tf_path):
                os.remove(tf_path)

    def test_no_modifiers_rejected(self):
        with self.assertRaises(ValueError):
            update_cosmic_shortcuts("/tmp/dummy", "/bin/nexus", [], "space")


if __name__ == "__main__":
    unittest.main()
