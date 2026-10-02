-- 掌阅专属（settings.reader.lua 补丁）。个人设置见 ../../personal/，这里只放随设备不同的。
return {
    -- 状态栏字体：京華老宋体 v3.0（三台统一，用户 2026-09-28 把掌阅上的 v2.0 换成了 v3.0）
    ["footer"] = { ["text_font_face"] = "/storage/emulated/0/koreader/fonts/京華老宋体v3.0.ttf" },
    -- 阅读背景：再生纸纹理（koreader/backgrounds/，apply.sh 拷到设备的 backgrounds/；用户 2026-09-29 要）。KOReader 菜单里没有这个设置，
    -- 只能写这里；路径要写设备上的绝对路径，所以放在设备层。不想要：删掉这一行再 apply（或 --uninstall 撤方案）。
    ["cre_background_image"] = "/storage/emulated/0/koreader/backgrounds/recycled-medium.png", -- 中档（用户 2026-09-29：浅的太淡）
}
