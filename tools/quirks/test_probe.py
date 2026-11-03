#!/usr/bin/env python3
"""probe.py 的离线单测（不碰设备）：python3 -m unittest tools/quirks/test_probe.py"""
import io
import json
import os
import sys
import tempfile
import unittest
from contextlib import redirect_stderr
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import probe  # noqa: E402


def fake_shot(path, n, b, shift=0):
    """合成一张 Kobo 截屏：顶上条码（宽 50%），下面三行黑条当字；`shift` 让第二行左边挪。"""
    from PIL import Image
    im = Image.new("L", (probe.KOBO_W, probe.KOBO_H), 255)
    code = Image.open(io.BytesIO(probe.code_png(n, b))).convert("L")
    w = probe.KOBO_W // 2
    im.paste(code.resize((w, w * code.height // code.width), Image.NEAREST), (100, 80))
    for i, y in enumerate((300, 360, 420)):
        x0 = 100 + (shift if i == 1 else 0)
        im.paste(0, (x0, y, 900, y + 30))
    im.paste(0, (500, 1420, 560, 1440))  # 页脚（--max-y 以下，不该算）
    im.save(path)


class Probe(unittest.TestCase):
    def test_usbms_base_like_booklib(self):
        self.assertEqual(probe.usbms_base({"BOOKLIB_USBMS_DIR": "/x", "USER": "u"}), "/x")
        self.assertEqual(probe.usbms_base({"USER": "u"}, exists=lambda p: True), "/run/media/u")
        self.assertEqual(probe.usbms_base({"USER": "u"}, exists=lambda p: p == "/media/u"), "/media/u")
        self.assertEqual(probe.usbms_base({"USER": "u"}, exists=lambda p: False), "/run/media/u")
        self.assertEqual(probe.usbms_base({"USER": ""}, exists=lambda p: p == "/media/"), "/run/media/")

    def test_mounted(self):
        with tempfile.TemporaryDirectory() as d:
            self.assertFalse(probe.mounted(d), "普通目录不是挂载点")
            self.assertFalse(probe.mounted(os.path.join(d, "无")))
        self.assertTrue(probe.mounted("/proc"))

    def test_kobo_mount_requires_real_mount(self):
        with tempfile.TemporaryDirectory() as d:
            os.mkdir(os.path.join(d, "KOBOeReader"))
            env = {"BOOKLIB_USBMS_DIR": d}
            with self.assertRaises(SystemExit):
                probe.kobo_mount(env)
            self.assertEqual(probe.kobo_mount(env, mount_check=False), os.path.join(d, "KOBOeReader"))
            os.rename(os.path.join(d, "KOBOeReader"), os.path.join(d, "KOBOeReader1"))
            with self.assertRaises(SystemExit):
                probe.kobo_mount(env, mount_check=False)

    def test_deliver_from_kobo_toml(self):
        self.assertEqual(probe.kobo_deliver(), ("KOBOeReader", "books"))

    def test_names(self):
        n = probe.kobo_names("1101120000")
        self.assertEqual(n, {"epub": "quirks-kb-1101120000.epub", "kepub": "quirks-kb-1101120000.kepub.epub"})
        self.assertNotEqual(n, probe.kobo_names("1101120001"))

    def test_safe_dest(self):
        self.assertEqual(probe.safe_dest("/m/K", "books/测试"), "/m/K/books/测试")
        for bad in (".kobo", ".kobo/x", "books/../.kobo", "../x", ".", "", "/etc/测试"):
            with self.assertRaises(SystemExit, msg=bad):
                probe.safe_dest("/m/K", bad)

    @mock.patch.object(probe, "MIN_Y", 0)  # 合成的是 Kobo 截屏（条码上沿 80）：同 kobo_main 不去页眉
    def test_codes_cover_all_probes(self):
        """条码能区分全部探针（7 位时 128 以后重号）。"""
        self.assertLess(len(probe.PROBES), 1 << probe.CODE_BITS)
        with tempfile.TemporaryDirectory() as d:
            for n in (1, 127, 128, len(probe.PROBES)):
                for b in (False, True):
                    p = os.path.join(d, "s.png")
                    fake_shot(p, n, b)
                    self.assertEqual(probe.decode_code(p), (n, b))

    @mock.patch.object(probe, "MIN_Y", 0)  # 同 kobo_main：Kobo 缺省不去页眉
    @mock.patch.object(probe, "MAX_Y", probe.KOBO_H * 95 // 100)
    def test_measure_from_dir(self):
        with tempfile.TemporaryDirectory() as d:
            fake_shot(os.path.join(d, "screen_001.png"), 13, False)
            fake_shot(os.path.join(d, "screen_002.png"), 13, True, shift=40)
            fake_shot(os.path.join(d, "screen_003.png"), 13, True, shift=99)  # 重复的一页：用前面那张
            fake_shot(os.path.join(d, "screen_004.png"), 1, False)
            with redirect_stderr(io.StringIO()):
                m = probe.kobo_measure_dir(d)
        self.assertEqual(set(m), {"13", "01"})
        self.assertEqual(len(m["13"]["A"]), 4, "条码 + 三行，页脚不算")
        r = dict((c, x) for c, _, x in probe.report(m))
        self.assertEqual(r["13"], "行2:上+0 左+40 右+0 高+0")
        self.assertEqual(r["01"], "没找到页")

    def test_run_probes_follow_generate(self):
        with tempfile.TemporaryDirectory() as d, mock.patch.object(probe, "PROBES", probe.ALL_PROBES):
            probe.save_run(d, {"only": "13,150", "collected": {}})
            probe.use_run_probes(d, {})
            self.assertEqual([p[0] for p in probe.PROBES], ["13", "150"])
            with self.assertRaises(SystemExit):
                probe.use_run_probes(d, {"--only": "13"})
            probe.save_run(d, {"only": None, "collected": {}})
            probe.use_run_probes(d, {})
            self.assertIs(probe.PROBES, probe.ALL_PROBES)

    @mock.patch.object(probe, "MIN_Y", 0)  # 同 kobo_main：Kobo 缺省不去页眉
    @mock.patch.object(probe, "MAX_Y", probe.KOBO_H * 95 // 100)
    def test_collect_incremental_and_clean(self):
        """拷过的不再拷；--clean 只删这次拷回来的；隐藏文件、符号链接、子目录、别的文件不碰。"""
        with tempfile.TemporaryDirectory() as base, tempfile.TemporaryDirectory() as run:
            dev = os.path.join(base, "KOBOeReader")
            os.makedirs(os.path.join(dev, ".kobo"))
            os.makedirs(os.path.join(dev, "sub"))
            fake_shot(os.path.join(dev, "screen_001.png"), 13, False)
            fake_shot(os.path.join(dev, ".screen_x.png"), 13, False)
            fake_shot(os.path.join(dev, "sub", "screen_009.png"), 13, False)
            fake_shot(os.path.join(dev, ".kobo", "screen_008.png"), 13, False)
            os.symlink(os.path.join(dev, ".kobo", "screen_008.png"), os.path.join(dev, "screen_link.png"))
            open(os.path.join(dev, "other.txt"), "w").close()
            probe.save_run(run, {"only": None, "collected": {}})
            env = {"BOOKLIB_USBMS_DIR": base, "BOOKLIB_MTP_NO_MOUNT_CHECK": "1"}
            argv = ["probe.py", "kobo", "collect", "--format=epub", "--pattern=*"]
            with mock.patch.dict(os.environ, env), mock.patch.object(sys, "argv", argv), redirect_stderr(io.StringIO()):
                probe.kobo_collect({"--run": run, "--pattern": "*"}, "epub")
                self.assertEqual(sorted(os.listdir(os.path.join(run, "shots", "epub"))), ["other.txt", "screen_001.png"])
                fake_shot(os.path.join(dev, "screen_002.png"), 13, True)
                with mock.patch.object(sys, "argv", argv + ["--clean"]):
                    probe.kobo_collect({"--run": run, "--pattern": "screen_*.png"}, "kepub")
            self.assertEqual(os.listdir(os.path.join(run, "shots", "kepub")), ["screen_002.png"], "screen_001 拷过了")
            left = sorted(os.listdir(dev))
            self.assertEqual(left, [".kobo", ".screen_x.png", "other.txt", "screen_001.png", "screen_link.png", "sub"])
            self.assertTrue(os.path.exists(os.path.join(dev, ".kobo", "screen_008.png")))
            with open(os.path.join(run, "run.json"), encoding="utf-8") as f:
                self.assertEqual(len(json.load(f)["collected"]["kepub"]), 1)

    def test_collect_same_name_new_content(self):
        """--clean 后设备重新编号：同名同大小、内容不同的截屏也要拷回来（去重按内容哈希）。"""
        with tempfile.TemporaryDirectory() as base, tempfile.TemporaryDirectory() as run:
            dev = os.path.join(base, "KOBOeReader")
            os.makedirs(dev)
            shot = os.path.join(dev, "screen_001.png")
            with open(shot, "wb") as f:
                f.write(b"A" * 64)
            probe.save_run(run, {"only": None, "collected": {}})
            env = {"BOOKLIB_USBMS_DIR": base, "BOOKLIB_MTP_NO_MOUNT_CHECK": "1"}
            argv = ["probe.py", "kobo", "collect", "--format=epub"]
            opts = {"--run": run, "--pattern": "screen_*.png"}
            with mock.patch.dict(os.environ, env), mock.patch.object(sys, "argv", argv), \
                    mock.patch.object(probe, "kobo_measure_dir"), redirect_stderr(io.StringIO()):
                probe.kobo_collect(opts, "epub")
                probe.kobo_collect(opts, "epub")  # 没变：不再拷
                with open(shot, "wb") as f:
                    f.write(b"B" * 64)
                probe.kobo_collect(opts, "epub")
                # 另一格式：设备上没清掉的上一格式截屏（名字、内容都一样）不拷；内容和上一格式某张一样但名字不同的要拷
                with open(os.path.join(dev, "screen_002.png"), "wb") as f:
                    f.write(b"A" * 64)
                probe.kobo_collect(opts, "kepub")
            with open(os.path.join(run, "run.json"), encoding="utf-8") as f:
                collected = json.load(f)["collected"]
            self.assertEqual(len(collected["epub"]), 2)
            self.assertEqual([k.split(":")[0] for k in collected["kepub"]], ["screen_002.png"])
            shots = os.path.join(run, "shots", "epub")
            names = sorted(os.listdir(shots))
            self.assertEqual(len(names), 2, "上一批的 screen_001 不被覆盖")
            with open(os.path.join(shots, "screen_001.png"), "rb") as f:
                self.assertEqual(f.read(), b"A" * 64)
            (renamed,) = [n for n in names if n != "screen_001.png"]
            self.assertRegex(renamed, r"^screen_001-[0-9a-f]{8}\.png$")
            with open(os.path.join(shots, renamed), "rb") as f:
                self.assertEqual(f.read(), b"B" * 64)

    def test_adb_checks_returncode_and_serial(self):
        """adb 失败就退出；指定了设备号加 -s。"""
        fail = mock.Mock(returncode=1, stdout=b"", stderr=b"no devices")
        with mock.patch.object(probe.subprocess, "run", return_value=fail) as run, \
                mock.patch.object(probe, "ADB_SERIAL", "X1"):
            with self.assertRaises(SystemExit):
                probe.adb("shell", "true")
            self.assertEqual(run.call_args[0][0][:3], ["adb", "-s", "X1"])
        ok = mock.Mock(returncode=0, stdout=b"hi", stderr=b"")
        with mock.patch.object(probe.subprocess, "run", return_value=ok), mock.patch.object(probe, "ADB_SERIAL", None):
            self.assertEqual(probe.adb("shell", "echo"), "hi")

    def test_clean_after_plain_collect_and_rerun_no_duplicate(self):
        """先不带 --clean 拷，再 --clean：拷过的也删（只删 .png）；拷了没记下就被打断的，重跑不重复拷。"""
        with tempfile.TemporaryDirectory() as base, tempfile.TemporaryDirectory() as run:
            dev = os.path.join(base, "KOBOeReader")
            os.makedirs(dev)
            for n, c in (("screen_001.png", b"A"), ("screen_002.png", b"B"), ("note.txt", b"C")):
                with open(os.path.join(dev, n), "wb") as f:
                    f.write(c * 64)
            probe.save_run(run, {"only": None, "collected": {}})
            # 模拟上次被打断：screen_001 已拷到电脑上、run.json 没记
            os.makedirs(os.path.join(run, "shots", "epub"))
            with open(os.path.join(run, "shots", "epub", "screen_001.png"), "wb") as f:
                f.write(b"A" * 64)
            env = {"BOOKLIB_USBMS_DIR": base, "BOOKLIB_MTP_NO_MOUNT_CHECK": "1"}
            argv = ["probe.py", "kobo", "collect", "--format=epub"]
            opts = {"--run": run, "--pattern": "*"}
            with mock.patch.dict(os.environ, env), mock.patch.object(sys, "argv", argv), \
                    mock.patch.object(probe, "kobo_measure_dir"), redirect_stderr(io.StringIO()):
                probe.kobo_collect(opts, "epub")
                self.assertEqual(sorted(os.listdir(os.path.join(run, "shots", "epub"))),
                                 ["note.txt", "screen_001.png", "screen_002.png"], "不重复拷")
                with mock.patch.object(sys, "argv", argv + ["--clean"]):
                    probe.kobo_collect(opts, "epub")
            self.assertEqual(os.listdir(dev), ["note.txt"], "拷过的 .png 都删，别的不碰")

    def test_generate_refuses_existing_run(self):
        with tempfile.TemporaryDirectory() as d:
            probe.save_run(d, {"collected": {"epub": ["x:1"]}})
            with self.assertRaises(SystemExit):
                probe.kobo_generate({"--run": d})


if __name__ == "__main__":
    unittest.main()
