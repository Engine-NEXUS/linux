#!/usr/bin/env python3
"""
NEXUS Document Downloader & Intelligent Analyzer.

Extracts text and structure from PDF and DOCX documents (using PyMuPDF & python-docx),
computes structural and qualitative analytics, and outputs both structured JSON
and a clean Markdown report for docking in the NEXUS Command Hub / Sidebar.
"""

import os
import sys
import json
import argparse
from pathlib import Path

def extract_pdf_text(filepath):
    import fitz  # PyMuPDF
    doc = fitz.open(filepath)
    pages = []
    total_text = ""
    for idx, page in enumerate(doc):
        text = page.get_text()
        pages.append({"page": idx + 1, "text": text})
        total_text += f"\n--- Page {idx + 1} ---\n" + text
    return {
        "format": "pdf",
        "page_count": len(doc),
        "text": total_text.strip(),
        "pages": pages
    }

def extract_docx_text(filepath):
    import docx
    doc = docx.Document(filepath)
    paragraphs = [p.text for p in doc.paragraphs if p.text.strip()]
    total_text = "\n".join(paragraphs)
    return {
        "format": "docx",
        "paragraph_count": len(paragraphs),
        "text": total_text.strip(),
        "paragraphs": paragraphs
    }

def classify_document(text):
    lower = text.lower()
    if any(k in lower for k in ["resume", "curriculum vitae", "education", "experience", "skills", "gpa"]):
        return "Resume / CV"
    if any(k in lower for k in ["internship", "stipend", "duration", "role", "offer letter"]):
        return "Internship / Job Posting"
    if any(k in lower for k in ["abstract", "methodology", "literature review", "results", "conclusion"]):
        return "Academic / Research Paper"
    if any(k in lower for k in ["architecture", "api", "database", "schema", "specification", "requirements"]):
        return "Technical Specification"
    if any(k in lower for k in ["agreement", "contract", "terms and conditions", "nda", "confidentiality"]):
        return "Legal Agreement"
    return "Project Document"

def evaluate_document(data, filename):
    text = data.get("text", "")
    words = text.split()
    word_count = len(words)
    doc_type = classify_document(text)

    # Key highlights & sections
    highlights = []
    lines = [l.strip() for l in text.split("\n") if l.strip()]
    for line in lines:
        if len(line) > 15 and len(line) < 120 and (line.endswith(":") or line.isupper() or line.startswith("#")):
            highlights.append(line.strip("#: "))

    # Strengths and recommendations
    strengths = []
    recommendations = []
    if word_count > 150:
        strengths.append("Comprehensive coverage with substantial detail.")
    else:
        recommendations.append("The document is brief; consider elaborating on specific technical metrics.")

    if any(k in text.lower() for k in ["python", "rust", "javascript", "react", "sql", "api"]):
        strengths.append("Contains concrete technical stack references and modern tooling.")
    
    if any(k in text.lower() for k in ["timeline", "milestone", "deadline", "phase"]):
        strengths.append("Includes clear scheduling or milestone indicators.")
    else:
        recommendations.append("Add explicit timelines and deliverable milestones.")

    rating = min(9.5, max(6.0, 7.0 + (0.5 if len(strengths) >= 2 else 0.0) + (1.0 if word_count > 300 else 0.0)))

    # Generate Markdown Summary Card
    md = []
    md.append(f"## Document Review: {filename}")
    md.append(f"**Classification:** `{doc_type}` | **Length:** {word_count} words | **Overall Rating:** **{rating:.1f}/10**\n")
    md.append("### Executive Summary")
    summary_preview = " ".join(words[:60]) + ("..." if len(words) > 60 else "")
    md.append(f"> {summary_preview}\n")
    
    if strengths:
        md.append("### Key Merits")
        for s in strengths:
            md.append(f"- [x] {s}")
        md.append("")

    if recommendations:
        md.append("### Recommendations")
        for r in recommendations:
            md.append(f"- [ ] {r}")
        md.append("")

    return {
        "filename": filename,
        "doc_type": doc_type,
        "word_count": word_count,
        "rating": rating,
        "strengths": strengths,
        "recommendations": recommendations,
        "markdown": "\n".join(md),
        "speech_summary": f"The document is a {doc_type} with approximately {word_count} words. I rate it {rating:.1f} out of 10. Overall, it demonstrates {strengths[0] if strengths else 'a solid foundation'}."
    }

def main():
    parser = argparse.ArgumentParser(description="NEXUS Document Analyzer")
    parser.add_argument("path", help="Path to document (PDF/DOCX) or directory")
    parser.add_argument("--json", action="store_true", help="Output JSON")
    args = parser.parse_args()

    target = Path(args.path)
    if not target.exists():
        res = {
            "error": f"File not found: {target}",
            "speech_summary": f"I could not find the file at {target}, sir.",
            "markdown": f"## Document Error\nFile not found: `{target}`"
        }
        print(json.dumps(res))
        return

    ext = target.suffix.lower()
    if ext == ".pdf":
        raw = extract_pdf_text(target)
    elif ext in [".docx", ".doc"]:
        raw = extract_docx_text(target)
    else:
        # Plain text
        raw = {"format": "text", "text": target.read_text(encoding="utf-8", errors="ignore")}

    result = evaluate_document(raw, target.name)
    print(json.dumps(result, indent=2))

if __name__ == "__main__":
    main()
