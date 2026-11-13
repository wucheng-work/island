"""将 assets/sounds 中的原音导出为 Windows 内置 PCM 资源和 Rust 目录。

在 macOS 上运行：python3 tools/make_windows_sounds.py
只使用 Python 标准库与系统 afconvert；Windows 构建直接使用已生成的 WAV。
"""

import array
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import wave


ROOT = Path(__file__).resolve().parent.parent
DEFAULTS = {
    "FOCUS": "modern-Pulse",
    "SHELF": "classic-Telegraph",
    "CODEX": "recent-Cheers",
    "WECHAT": "classic-Tri-Tone",
    "MAIL": "modern-Bamboo",
}


def import_sound(source, output):
    with tempfile.TemporaryDirectory() as directory:
        decoded = Path(directory) / "decoded.wav"
        subprocess.run(["afconvert", "-f", "WAVE", "-d", "LEI16", str(source), str(decoded)], check=True)
        data = decoded.read_bytes()
    if data[:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise ValueError("afconvert did not produce a WAV file")
    position = 12
    format_data = None
    pcm = None
    while position + 8 <= len(data):
        kind = data[position:position + 4]
        size = struct.unpack_from("<I", data, position + 4)[0]
        body = data[position + 8:position + 8 + size]
        if kind == b"fmt ":
            format_data = body
        elif kind == b"data":
            pcm = body
        position += 8 + size + size % 2
    if format_data is None or pcm is None:
        raise ValueError("WAV format or audio data is missing")
    format_id, channels, rate, _, _, bits = struct.unpack_from("<HHIIHH", format_data)
    if format_id == 65534 and format_data[24:40] == bytes.fromhex("0100000000001000800000aa00389b71"):
        format_id = 1
    if format_id != 1 or bits != 16:
        raise ValueError("Expected 16-bit PCM audio")
    samples = array.array("h", pcm)
    if sys.byteorder != "little":
        samples.byteswap()
    peak = max(map(abs, samples), default=0)
    if peak > 0:
        gain = 0.7 * 32767 / peak
        samples = array.array("h", (round(sample * gain) for sample in samples))
    if sys.byteorder != "little":
        samples.byteswap()
    # 统一峰值并保留余量，保留原音内容；去掉扩展 WAV 头以兼容 WinMM。
    with wave.open(str(output), "wb") as destination:
        destination.setnchannels(channels)
        destination.setsampwidth(2)
        destination.setframerate(rate)
        destination.writeframes(samples.tobytes())


if __name__ == "__main__":
    if len(sys.argv) != 1:
        raise SystemExit(__doc__)
    catalog = json.loads((ROOT / "assets/sounds/catalog.json").read_text())
    output = ROOT / "crates/poni/res/sounds"
    output.mkdir(exist_ok=True)
    metadata = [
        "// 由 tools/make_windows_sounds.py 生成，请修改目录源文件后重新生成。",
        "#[derive(Clone, Copy, Debug)]",
        "pub struct BuiltinSound {",
        "    pub id: &'static str,",
        "    pub name: &'static str,",
        "    pub category: &'static str,",
        "}",
        "",
    ]
    for module, identifier in DEFAULTS.items():
        metadata.append(f"pub const DEFAULT_{module}: &str = {json.dumps(identifier)};")
    metadata.extend(["", "pub const SOUNDS: &[BuiltinSound] = &["])
    audio = [
        "// 由 tools/make_windows_sounds.py 生成，请修改目录源文件后重新生成。",
        "pub const SOUNDS: &[(&str, &[u8])] = &[",
    ]
    for entry in catalog:
        identifier = entry["id"]
        import_sound(ROOT / "assets/sounds" / entry["file"], output / f"{identifier}.wav")
        values = [json.dumps(value, ensure_ascii=False) for value in (identifier, entry["name"], entry["group"])]
        metadata.append(f"    BuiltinSound {{ id: {values[0]}, name: {values[1]}, category: {values[2]} }},")
        audio.append(f'    ({values[0]}, include_bytes!("../../res/sounds/{identifier}.wav")),')
    metadata.extend([
        "];", "",
        "pub fn find(id: &str) -> Option<&'static BuiltinSound> {",
        "    SOUNDS.iter().find(|sound| sound.id == id)",
        "}",
    ])
    audio.extend([
        "];", "",
        "pub fn find(id: &str) -> Option<&'static [u8]> {",
        "    SOUNDS.iter().find(|(key, _)| *key == id).map(|(_, bytes)| *bytes)",
        "}",
    ])
    (ROOT / "crates/poni-core/src/sound_catalog.rs").write_text("\n".join(metadata) + "\n")
    (ROOT / "crates/poni/src/services/sound_data.rs").write_text("\n".join(audio) + "\n")
