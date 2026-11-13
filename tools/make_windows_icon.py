"""用已选定的小岛原图生成 Windows 多尺寸图标。

运行：python3 tools/make_windows_icon.py [output.ico]，需要现有的 Pillow。
"""

import sys
from pathlib import Path

from PIL import Image

from make_mac_icon import ROOT, artwork


def main() -> int:
    if len(sys.argv) > 2:
        print(__doc__)
        return 2
    out = Path(sys.argv[1]) if len(sys.argv) == 2 else ROOT / "crates/poni/res/poni.ico"
    out.parent.mkdir(parents=True, exist_ok=True)
    image = artwork().resize((256, 256), Image.Resampling.LANCZOS)
    sizes = [(size, size) for size in (16, 20, 24, 32, 40, 48, 64, 96, 128, 256)]
    image.save(out, format="ICO", sizes=sizes)
    return 0


if __name__ == "__main__":
    sys.exit(main())
