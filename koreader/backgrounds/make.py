#!/usr/bin/env python3
"""生成 KOReader 阅读背景：再生纸纹理（可无缝平铺的灰度 PNG）。用法: python3 make.py（写出同目录的 recycled-*.png）

墨水屏是 16 级灰度（0、17、34……238、255），中间值会被屏幕控制器抖动成斑块。所以：
- 底色保持纯白 255：不降低文字对比度；
- 纤维、杂点只用屏幕能精确显示的几级浅灰（238、221，少量 204），画出来就是那个灰，不抖动；
- 平铺无缝：所有笔画按图块尺寸取模（从右边出去的从左边进来）。
KOReader 按 settings.reader.lua 的 cre_background_image 平铺在正文下面（只对 EPUB 等流式排版的书有效）。
需要 PIL（Pillow）。随机数固定种子，每次生成的图一样。
"""
import math
import random
from PIL import Image

SIZE = 512
LEVELS = (238, 221, 204)  # 屏幕能精确显示的浅灰（255 - 17k）


def put(px, x, y, v):
    x %= SIZE
    y %= SIZE
    if v < px[x, y]:  # 叠加时取更深的
        px[x, y] = v


def fiber(px, rnd, level):
    """一根纤维：一段弯曲的细线，长 12–40 像素。"""
    x, y = rnd.uniform(0, SIZE), rnd.uniform(0, SIZE)
    a = rnd.uniform(0, math.pi)
    length = rnd.randint(12, 40)
    bend = rnd.uniform(-0.08, 0.08)
    for _ in range(length):
        put(px, int(x), int(y), level)
        x += math.cos(a)
        y += math.sin(a)
        a += bend


def fleck(px, rnd, level):
    """一个杂点：1–3 像素的小团。"""
    x, y = rnd.randrange(SIZE), rnd.randrange(SIZE)
    for _ in range(rnd.randint(1, 4)):
        put(px, x + rnd.randint(-1, 1), y + rnd.randint(-1, 1), level)


def make(name, fibers, flecks, dark_flecks, seed):
    rnd = random.Random(seed)
    img = Image.new("L", (SIZE, SIZE), 255)
    px = img.load()
    for _ in range(fibers):
        fiber(px, rnd, LEVELS[0] if rnd.random() < 0.8 else LEVELS[1])
    for _ in range(flecks):
        fleck(px, rnd, LEVELS[0] if rnd.random() < 0.7 else LEVELS[1])
    for _ in range(dark_flecks):
        fleck(px, rnd, LEVELS[2])
    img.save(name, optimize=True)
    dark = 1 - img.histogram()[255] / (SIZE * SIZE)
    print(f"{name}: {SIZE}×{SIZE}，非白像素 {dark:.1%}")


make("recycled-light.png", fibers=90, flecks=160, dark_flecks=6, seed=1)
make("recycled-medium.png", fibers=220, flecks=420, dark_flecks=25, seed=2)
