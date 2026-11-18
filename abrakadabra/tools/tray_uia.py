import sys
import uiautomation as auto

sys.stdout.reconfigure(encoding="utf-8")

taskbar = auto.Control(Name="Taskbar", searchDepth=3)
if not taskbar.Exists(5):
    print("no taskbar control found")
    sys.exit(1)


def walk(ctrl, depth=0, max_depth=9):
    if depth > max_depth:
        return
    for c in ctrl.GetChildren():
        try:
            name = c.Name
        except Exception:
            name = "?"
        ctype = c.ControlTypeName
        if name or ctype in ("ButtonControl", "ImageControl", "MenuControl", "MenuItemControl"):
            print(f"{'  ' * depth}{ctype} | {name!r}")
        walk(c, depth + 1, max_depth)


walk(taskbar)
