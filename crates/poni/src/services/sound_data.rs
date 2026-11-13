// 由 tools/make_windows_sounds.py 生成，请修改目录源文件后重新生成。
pub const SOUNDS: &[(&str, &[u8])] = &[
    ("recent-Antic", include_bytes!("../../res/sounds/recent-Antic.wav")),
    ("recent-Cheers", include_bytes!("../../res/sounds/recent-Cheers.wav")),
    ("recent-Droplet", include_bytes!("../../res/sounds/recent-Droplet.wav")),
    ("recent-Handoff", include_bytes!("../../res/sounds/recent-Handoff.wav")),
    ("recent-Milestone", include_bytes!("../../res/sounds/recent-Milestone.wav")),
    ("recent-Passage", include_bytes!("../../res/sounds/recent-Passage.wav")),
    ("recent-Portal", include_bytes!("../../res/sounds/recent-Portal.wav")),
    ("recent-Rattle", include_bytes!("../../res/sounds/recent-Rattle.wav")),
    ("recent-Rebound", include_bytes!("../../res/sounds/recent-Rebound.wav")),
    ("recent-Slide", include_bytes!("../../res/sounds/recent-Slide.wav")),
    ("recent-Welcome", include_bytes!("../../res/sounds/recent-Welcome.wav")),
    ("modern-Aurora", include_bytes!("../../res/sounds/modern-Aurora.wav")),
    ("modern-Bamboo", include_bytes!("../../res/sounds/modern-Bamboo.wav")),
    ("modern-Chord", include_bytes!("../../res/sounds/modern-Chord.wav")),
    ("modern-Circles", include_bytes!("../../res/sounds/modern-Circles.wav")),
    ("modern-Complete", include_bytes!("../../res/sounds/modern-Complete.wav")),
    ("modern-Hello", include_bytes!("../../res/sounds/modern-Hello.wav")),
    ("modern-Input", include_bytes!("../../res/sounds/modern-Input.wav")),
    ("modern-Keys", include_bytes!("../../res/sounds/modern-Keys.wav")),
    ("modern-Note", include_bytes!("../../res/sounds/modern-Note.wav")),
    ("modern-Popcorn", include_bytes!("../../res/sounds/modern-Popcorn.wav")),
    ("modern-Pulse", include_bytes!("../../res/sounds/modern-Pulse.wav")),
    ("modern-Synth", include_bytes!("../../res/sounds/modern-Synth.wav")),
    ("classic-Telegraph", include_bytes!("../../res/sounds/classic-Telegraph.wav")),
    ("classic-Tri-Tone", include_bytes!("../../res/sounds/classic-Tri-Tone.wav")),
];

pub fn find(id: &str) -> Option<&'static [u8]> {
    SOUNDS.iter().find(|(key, _)| *key == id).map(|(_, bytes)| *bytes)
}
