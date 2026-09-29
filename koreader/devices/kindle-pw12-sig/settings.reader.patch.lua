-- Kindle 专属（settings.reader.lua 补丁）。个人设置见 ../../personal/，这里只放随设备不同的。
return {
    -- 状态栏字体：京華老宋体，用 Kindle 上的 v3.0（字体族名 KingHwaOldSong；掌阅上是 v2.0 的另一个文件，用户 2026-09-28 选定）
    ["footer"] = { ["text_font_face"] = "./fonts/京華老宋体v3.0.ttf" },
    -- 阅读背景：再生纸纹理（koreader/backgrounds/，apply.sh 拷到设备的 backgrounds/；用户 2026-09-29 要）。KOReader 菜单里没有这个设置，
    -- 只能写这里；路径要写设备上的绝对路径，所以放在设备层。不想要：删掉这一行再 apply（或 --uninstall 撤方案）。
    ["cre_background_image"] = "/mnt/us/koreader/backgrounds/recycled-medium.png", -- 中档（用户 2026-09-29：浅的太淡）
}
