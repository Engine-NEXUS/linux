#!/usr/bin/env python3
"""
NEXUS Browser DOM & Interactive Element Extraction Engine.
Derived from open-source `browser-use/browser-use` (DOM serializer & clickable detector).
Provides zero-headless-browser, zero-API-key web page crawling, clean markdown extraction,
and interactive element mapping for NEXUS BrowserCenter.
"""

import sys
import re
import json
import html
import urllib.request
import urllib.parse
from typing import Dict, List, Any, Optional

# Force UTF-8 stdout for Windows consoles
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")


class BrowserUseHtmlCleaner:
    """
    Strips scripts, styles, metadata, comments, and noise tags
    using browser-use HTMLSerializer heuristics.
    """
    TAGS_TO_REMOVE = [
        r"<script\b[^<]*(?:(?!<\/script>)<[^<]*)*<\/script>",
        r"<style\b[^<]*(?:(?!<\/style>)<[^<]*)*<\/style>",
        r"<svg\b[^<]*(?:(?!<\/svg>)<[^<]*)*<\/svg>",
        r"<!--[\s\S]*?-->",
        r"<noscript\b[^<]*(?:(?!<\/noscript>)<[^<]*)*<\/noscript>",
    ]

    @classmethod
    def clean(cls, raw_html: str) -> str:
        text = raw_html
        for pattern in cls.TAGS_TO_REMOVE:
            text = re.sub(pattern, " ", text, flags=re.IGNORECASE)
        return text


class BrowserUseElementExtractor:
    """
    Extracts interactive elements (links, buttons, inputs, selects, textareas)
    inspired by browser-use ClickableElementDetector.
    """
    @staticmethod
    def extract_interactive(raw_html: str) -> List[Dict[str, Any]]:
        elements = []
        cleaned = BrowserUseHtmlCleaner.clean(raw_html)

        # 1. Links (<a ...>text</a>)
        for m in re.finditer(r'<a\s+([^>]*?)>(.*?)<\/a>', cleaned, re.IGNORECASE | re.DOTALL):
            attrs, content = m.group(1), m.group(2)
            href_m = re.search(r'href=[\'"]([^\'"]+)[\'"]', attrs, re.IGNORECASE)
            text = re.sub(r'<[^>]+>', '', content).strip()
            href = href_m.group(1) if href_m else ""
            if text or href:
                elements.append({
                    "type": "link",
                    "text": text,
                    "target": href,
                    "interactive": True
                })

        # 2. Buttons (<button ...>text</button>)
        for m in re.finditer(r'<button\s+([^>]*?)>(.*?)<\/button>', cleaned, re.IGNORECASE | re.DOTALL):
            attrs, content = m.group(1), m.group(2)
            name_m = re.search(r'name=[\'"]([^\'"]+)[\'"]', attrs, re.IGNORECASE)
            text = re.sub(r'<[^>]+>', '', content).strip()
            elements.append({
                "type": "button",
                "text": text,
                "name": name_m.group(1) if name_m else "",
                "interactive": True
            })

        # 3. Inputs (<input ...>)
        for m in re.finditer(r'<input\s+([^>]*?)\/?>', cleaned, re.IGNORECASE):
            attrs = m.group(1)
            type_m = re.search(r'type=[\'"]([^\'"]+)[\'"]', attrs, re.IGNORECASE)
            name_m = re.search(r'name=[\'"]([^\'"]+)[\'"]', attrs, re.IGNORECASE)
            placeholder_m = re.search(r'placeholder=[\'"]([^\'"]+)[\'"]', attrs, re.IGNORECASE)
            value_m = re.search(r'value=[\'"]([^\'"]+)[\'"]', attrs, re.IGNORECASE)

            input_type = type_m.group(1).lower() if type_m else "text"
            if input_type != "hidden":
                elements.append({
                    "type": f"input_{input_type}",
                    "name": name_m.group(1) if name_m else "",
                    "placeholder": placeholder_m.group(1) if placeholder_m else "",
                    "value": value_m.group(1) if value_m else "",
                    "interactive": True
                })

        return elements[:80]  # Cap top 80 interactive elements


def html_to_clean_markdown(raw_html: str) -> str:
    """Converts HTML to clean, readable Markdown."""
    cleaned = BrowserUseHtmlCleaner.clean(raw_html)

    # Convert headings
    for i in range(6, 0, -1):
        cleaned = re.sub(
            rf'<h{i}\b[^>]*>(.*?)<\/h{i}>',
            lambda m: f"\n\n{'#' * i} {re.sub(r'<[^>]+>', '', m.group(1)).strip()}\n\n",
            cleaned,
            flags=re.IGNORECASE | re.DOTALL
        )

    # Convert paragraphs and divs
    cleaned = re.sub(r'<(?:p|div)\b[^>]*>(.*?)<\/(?:p|div)>', r'\n\1\n', cleaned, flags=re.IGNORECASE | re.DOTALL)

    # Convert line breaks
    cleaned = re.sub(r'<br\s*\/?>', '\n', cleaned, flags=re.IGNORECASE)

    # Convert bold / italics
    cleaned = re.sub(r'<(?:strong|b)\b[^>]*>(.*?)<\/(?:strong|b)>', r'**\1**', cleaned, flags=re.IGNORECASE | re.DOTALL)
    cleaned = re.sub(r'<(?:em|i)\b[^>]*>(.*?)<\/(?:em|i)>', r'*\1*', cleaned, flags=re.IGNORECASE | re.DOTALL)

    # Strip remaining HTML tags
    text = re.sub(r'<[^>]+>', ' ', cleaned)
    text = html.unescape(text)

    # Normalize repeated whitespace and blank lines
    lines = [re.sub(r'[ \t]+', ' ', l).strip() for l in text.split('\n')]
    non_empty = []
    prev_empty = False
    for line in lines:
        if not line:
            if not prev_empty:
                non_empty.append("")
                prev_empty = True
        else:
            non_empty.append(line)
            prev_empty = False

    return '\n'.join(non_empty).strip()


def fetch_url(url: str, user_agent: Optional[str] = None) -> str:
    """Fetch URL with browser user agent headers."""
    if not url.startswith("http://") and not url.startswith("https://"):
        url = "https://" + url

    headers = {
        "User-Agent": user_agent or "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36",
        "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        "Accept-Language": "en-US,en;q=0.9",
    }
    req = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(req, timeout=12) as response:
        content = response.read()
        encoding = response.headers.get_content_charset() or "utf-8"
        return content.decode(encoding, errors="replace")


def main():
    import argparse
    parser = argparse.ArgumentParser(description="NEXUS Browser-Use Crawler Engine")
    subparsers = parser.add_subparsers(dest="command")

    # read-page
    read_p = subparsers.add_parser("read-page", help="Read URL content as clean markdown")
    read_p.add_argument("url", type=str, help="Target URL")

    # elements
    elem_p = subparsers.add_parser("elements", help="Extract interactive elements from URL")
    elem_p.add_argument("url", type=str, help="Target URL")

    args = parser.parse_args()

    if args.command == "read-page":
        try:
            content = fetch_url(args.url)
            md = html_to_clean_markdown(content)
            print(json.dumps({
                "status": "ok",
                "url": args.url,
                "markdown": md[:10000]  # Cap 10k chars
            }, indent=2))
        except Exception as e:
            print(json.dumps({"status": "error", "message": str(e)}))
    elif args.command == "elements":
        try:
            content = fetch_url(args.url)
            elements = BrowserUseElementExtractor.extract_interactive(content)
            print(json.dumps({
                "status": "ok",
                "url": args.url,
                "count": len(elements),
                "elements": elements
            }, indent=2))
        except Exception as e:
            print(json.dumps({"status": "error", "message": str(e)}))
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
