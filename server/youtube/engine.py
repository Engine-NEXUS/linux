"""YouTube Engine Bridge — CLI & Native Tool Executor for NEXUS.

Exposes YouTube capabilities (Search, Metadata, Transcript, Channel Search,
and Video Journal Summarization) via stdio JSON or Python API.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

if sys.platform == "win32":
    try:
        sys.stdout.reconfigure(encoding="utf-8")
        sys.stderr.reconfigure(encoding="utf-8")
    except Exception:
        pass

try:
    from . import transcripts, youtube
    from .models import TranscriptSegment, Video
except (ImportError, ValueError):
    import transcripts, youtube
    from models import TranscriptSegment, Video


def search(query: str, limit: int = 5) -> list[dict]:
    """Search YouTube videos and return compact JSON-ready dictionaries."""
    videos = youtube.search_videos(query, limit=limit)
    return [
        {
            "id": v.id,
            "title": v.title,
            "url": v.url,
            "duration": v.duration,
            "channel": v.channel,
            "views": v.views,
        }
        for v in videos
    ]


def get_transcript(video_url: str, language: str = "en") -> dict:
    """Fetch timestamped transcript segments for a video."""
    segments = transcripts.get_transcript(video_url, language=language)
    full_text = " ".join(s.text for s in segments)
    return {
        "url": video_url,
        "segment_count": len(segments),
        "full_text": full_text,
        "segments": [s.to_dict() for s in segments[:100]],  # cap sample
    }


def summarize_for_journal(video_url: str) -> dict:
    """Extract video info + transcript and generate structured journal entry data."""
    info = youtube.get_video_info(video_url)
    try:
        segments = transcripts.get_transcript(video_url, language="en")
        full_text = " ".join(s.text for s in segments)
    except Exception as e:
        full_text = f"(Transcript unavailable: {e})"
        segments = []

    # Build structured journal digest
    preview_len = min(len(full_text), 1500)
    journal_entry = {
        "title": info.title,
        "channel": info.channel,
        "url": info.url,
        "duration": info.duration,
        "views": info.views,
        "description": (info.description or "")[:300],
        "transcript_excerpt": full_text[:preview_len],
        "is_full_transcript": len(full_text) <= 1500,
    }
    return journal_entry


def main():
    parser = argparse.ArgumentParser(description="NEXUS YouTube Engine")
    subparsers = parser.add_subparsers(dest="action", required=True)

    # Search
    search_parser = subparsers.add_parser("search")
    search_parser.add_argument("query", type=str, help="Search query")
    search_parser.add_argument("--limit", type=int, default=5, help="Max results")

    # Transcript
    transcript_parser = subparsers.add_parser("transcript")
    transcript_parser.add_argument("url", type=str, help="YouTube video URL")
    transcript_parser.add_argument("--lang", type=str, default="en", help="Language code")

    # Journal
    journal_parser = subparsers.add_parser("journal")
    journal_parser.add_argument("url", type=str, help="YouTube video URL")

    args = parser.parse_args()

    try:
        if args.action == "search":
            results = search(args.query, limit=args.limit)
            print(json.dumps({"ok": True, "data": results}, ensure_ascii=False, indent=2))
        elif args.action == "transcript":
            res = get_transcript(args.url, language=args.lang)
            print(json.dumps({"ok": True, "data": res}, ensure_ascii=False, indent=2))
        elif args.action == "journal":
            res = summarize_for_journal(args.url)
            print(json.dumps({"ok": True, "data": res}, ensure_ascii=False, indent=2))
    except Exception as err:
        print(json.dumps({"ok": False, "error": str(err)}, ensure_ascii=False), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
