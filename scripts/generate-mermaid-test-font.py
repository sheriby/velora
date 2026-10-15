#!/usr/bin/env python3
"""生成合成测试字体；重新生成时需安装 fonttools，不读取或复制系统字体。"""

from pathlib import Path

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen


characters = sorted(set("创建订单校验库存"))
character_map = {ord(character): f"uni{ord(character):04X}" for character in characters}
character_map[32] = "space"
glyph_order = [".notdef", "space", *[character_map[ord(character)] for character in characters]]
builder = FontBuilder(1000, isTTF=True)
builder.setupGlyphOrder(glyph_order)
builder.setupCharacterMap(character_map)

# 矩形字形只验证字体选择和中文字符覆盖，不是供界面使用的字形设计。
glyphs = {}
for name in glyph_order:
    pen = TTGlyphPen(None)
    if name not in [".notdef", "space"]:
        pen.moveTo((100, 0))
        pen.lineTo((800, 0))
        pen.lineTo((800, 700))
        pen.lineTo((100, 700))
        pen.closePath()
    glyphs[name] = pen.glyph()

builder.setupGlyf(glyphs)
builder.setupHorizontalMetrics({name: (1000, 0) for name in glyph_order})
builder.setupHorizontalHeader(ascent=800, descent=-200)
builder.setupNameTable({
    "familyName": "Velora Mermaid Test",
    "styleName": "Regular",
    "uniqueFontIdentifier": "VeloraMermaidTest-Regular-1",
    "fullName": "Velora Mermaid Test Regular",
    "psName": "VeloraMermaidTest-Regular",
    "version": "Version 1.000",
    "copyright": "Synthetic test glyphs, licensed under Apache-2.0.",
})
builder.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
builder.setupPost()
builder.setupMaxp()
builder.setupHead(created=2082844800, modified=2082844800)
builder.font.recalcTimestamp = False
destination = Path(__file__).resolve().parent.parent / "fixtures/fonts/mermaid-cjk-test.ttf"
destination.parent.mkdir(parents=True, exist_ok=True)
builder.save(destination)
