#!/bin/sh
# 清掉 Kindle 自动生成的崩溃诊断包（用户 2026-10-02 要）：停亚马逊界面时，主界面程序 KPPMainAppV2 偶尔在关闭途中崩溃，
# 系统的崩溃处理（dmcc）就往 documents/ 写一份「Oct_02_10.06.28_2026.txt」（摘要）、「….tgz」（完整日志，约 3MB），
# Kindle 还会给 .txt 建一个「….sdr」目录。书库里看着像一本书，没用。
# 只删名字完全是「月_日_时.分.秒_年」的这几样，别的文件不碰。开机自启和「KOReader（独占）」在启动 KOReader 前后各调一次
# （.tgz 要停界面后一两分钟才写完，启动前那次清不到这一回的，退出后那次清）。
cd /mnt/us/documents 2>/dev/null || exit 0
pat='[A-Z][a-z][a-z]_[0-3][0-9]_[0-2][0-9].[0-5][0-9].[0-5][0-9]_[12][0-9][0-9][0-9]'
for f in $pat.txt $pat.tgz; do
    [ -f "$f" ] && rm -f "$f"
done
for d in $pat.sdr; do
    [ -d "$d" ] && rm -rf "$d"
done
exit 0
