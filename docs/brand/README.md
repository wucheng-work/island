# 灵动岛品牌文件

当前本地定制版本的可见名称为“灵动岛”。应用图标是石墨色底上的蓝青色悬浮岛和两张文件卡片。部分资源路径沿用旧文件名，避免打包脚本和历史引用失效。

| 文件 | 用途 |
|---|---|
| `oyen-poni-logo.svg`、`oyen-poni-logo-512.png` | 当前应用图标和文档图标 |
| `../../crates/poni/res/poni.ico` | Windows 图标 |
| `../../installer/art/wizard-small-*.png`、`nsis-header.bmp` | 安装器使用的生成图片 |
| `oyen-mark.svg`、`oyen-mark-white.svg` | 保留的上游 Oyen 标记；主题中的 Oyen 角色继续使用 |
| `art/`、`readme/`、`social-preview.png` | 上游宣传和历史截图素材；可能包含本地版本已删除的页面，不作为当前功能说明 |

图标源头是 `tools/make_brand.py`。运行它可生成 SVG、PNG、ICO 和安装器图片：

```sh
python3 tools/make_brand.py
```

Mac 图标由 `tools/make_mac_icon.py` 使用同一绘制源头按尺寸生成 iconset，再由 `iconutil` 生成 `.icns`。打包时资源名仍可使用 `OyenzaStudio.icns`，用户可见应用包名为 `灵动岛.app`。

Oyen 是保留的上游 Oyenza 橙猫角色。绘制时沿用原标记的头型、折耳和双眼，不增加嘴、鼻、胡须或腮红。保留角色与品牌素材不表示本地定制属于上游官方发行。

这些品牌素材不在代码的 MIT 许可范围内。历史发行材料保留原名称和对应权利说明。
