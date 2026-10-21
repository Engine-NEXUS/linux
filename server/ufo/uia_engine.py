#!/usr/bin/env python3
"""
UFO Windows UI Automation Engine Bridge for NEXUS.
Derived from Microsoft UFO (UI-Focused Agent for Windows OS).
Extracts interactive UI elements, control hierarchies, and bounding rects.
"""

import sys
import json
import argparse
from typing import Dict, List, Any, Optional

# Force UTF-8 stdout for Windows consoles
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")


def get_desktop_windows_win32() -> List[Dict[str, Any]]:
    """Enumerate visible desktop application windows using ctypes."""
    import ctypes
    from ctypes import wintypes

    user32 = ctypes.windll.user32
    windows = []

    WNDENUMPROC = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user32.EnumWindows.argtypes = [WNDENUMPROC, wintypes.LPARAM]
    user32.EnumWindows.restype = wintypes.BOOL
    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    user32.IsWindowVisible.restype = wintypes.BOOL
    user32.GetWindowTextLengthW.argtypes = [wintypes.HWND]
    user32.GetWindowTextLengthW.restype = ctypes.c_int
    user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    user32.GetWindowTextW.restype = ctypes.c_int

    def enum_windows_callback(hwnd, lparam):
        if not user32.IsWindowVisible(hwnd):
            return True
        length = user32.GetWindowTextLengthW(hwnd)
        if length == 0:
            return True
        buff = ctypes.create_unicode_buffer(length + 1)
        user32.GetWindowTextW(hwnd, buff, length + 1)
        title = buff.value.strip()
        if not title:
            return True

        rect = wintypes.RECT()
        user32.GetWindowRect(hwnd, ctypes.byref(rect))
        w = rect.right - rect.left
        h = rect.bottom - rect.top
        if w > 50 and h > 50:
            windows.append({
                "hwnd": int(hwnd),
                "title": title,
                "rect": {
                    "left": rect.left,
                    "top": rect.top,
                    "right": rect.right,
                    "bottom": rect.bottom,
                    "width": w,
                    "height": h,
                }
            })
        return True

    user32.EnumWindows(WNDENUMPROC(enum_windows_callback), 0)
    return windows


def inspect_controls_uia(hwnd: Optional[int] = None, title_query: Optional[str] = None) -> Dict[str, Any]:
    """
    Inspect interactive controls for a window using UI Automation.
    Follows Microsoft UFO control filter rules:
    - Filters interactive control types (Button, Edit, ComboBox, MenuItem, TabItem, Hyperlink, CheckBox, RadioButton)
    - Extracts screen coordinates [left, top, right, bottom] and center points
    """
    try:
        import uiautomation as auto
    except ImportError:
        # Fallback to win32 window bounds if uiautomation is not installed
        windows = get_desktop_windows_win32()
        target = None
        for w in windows:
            if hwnd and w["hwnd"] == hwnd:
                target = w
                break
            if title_query and title_query.lower() in w["title"].lower():
                target = w
                break
        return {
            "status": "partial",
            "message": "uiautomation package not installed; returning top-level window bounds",
            "window": target,
            "controls": []
        }

    target_control = None
    if hwnd:
        target_control = auto.ControlFromHandle(hwnd)
    elif title_query:
        target_control = auto.WindowControl(searchDepth=1, SubName=title_query)
    else:
        # Foreground window
        target_control = auto.GetForegroundControl()

    if not target_control or not target_control.Exists(0, 0):
        return {"status": "error", "message": "Target window not found", "controls": []}

    rect = target_control.BoundingRectangle
    win_info = {
        "name": target_control.Name,
        "control_type": target_control.ControlTypeName,
        "rect": {
            "left": rect.left,
            "top": rect.top,
            "right": rect.right,
            "bottom": rect.bottom,
            "width": rect.width(),
            "height": rect.height(),
        }
    }

    interactive_types = {
        "ButtonControl", "EditControl", "ComboBoxControl", "MenuItemControl",
        "TabItemControl", "HyperlinkControl", "CheckBoxControl", "RadioButtonControl",
        "ListItemControl", "TreeItemControl", "DocumentControl"
    }

    controls = []
    try:
        for control, depth in auto.WalkControl(target_control, maxDepth=4):
            c_type = control.ControlTypeName
            name = control.Name.strip() if control.Name else ""
            c_rect = control.BoundingRectangle

            # Skip controls with zero or negative area
            if c_rect.width() <= 0 or c_rect.height() <= 0:
                continue

            # Check interactivity or meaningful text
            if c_type in interactive_types or (name and c_rect.width() > 10 and c_rect.height() > 10):
                center_x = (c_rect.left + c_rect.right) // 2
                center_y = (c_rect.top + c_rect.bottom) // 2
                controls.append({
                    "id": f"ctrl_{len(controls) + 1}",
                    "name": name,
                    "type": c_type.replace("Control", ""),
                    "interactive": c_type in interactive_types,
                    "rect": {
                        "left": c_rect.left,
                        "top": c_rect.top,
                        "right": c_rect.right,
                        "bottom": c_rect.bottom,
                        "width": c_rect.width(),
                        "height": c_rect.height(),
                    },
                    "center": [center_x, center_y],
                    "depth": depth
                })
    except Exception as e:
        pass

    return {
        "status": "ok",
        "window": win_info,
        "count": len(controls),
        "controls": controls[:100]  # Cap top 100 interactive elements
    }


def main():
    parser = argparse.ArgumentParser(description="NEXUS UFO UI Automation Engine")
    subparsers = parser.add_subparsers(dest="command")

    subparsers.add_parser("list-windows", help="List visible desktop windows")

    inspect_p = subparsers.add_parser("inspect", help="Inspect controls of window")
    inspect_p.add_argument("--hwnd", type=int, help="Window HWND")
    inspect_p.add_argument("--title", type=str, help="Window title query")

    args = parser.parse_args()

    if args.command == "list-windows":
        windows = get_desktop_windows_win32()
        print(json.dumps(windows, indent=2))
    elif args.command == "inspect":
        res = inspect_controls_uia(hwnd=args.hwnd, title_query=args.title)
        print(json.dumps(res, indent=2))
    else:
        parser.print_help()


if __name__ == "__main__":
    main()
