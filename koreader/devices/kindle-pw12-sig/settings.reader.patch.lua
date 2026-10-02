-- Kindle 专属（settings.reader.lua 补丁）。个人设置见 ../../personal/，这里只放随设备不同的。
return {
    -- 状态栏字体：京華老宋体，用 Kindle 上的 v3.0（字体族名 KingHwaOldSong；掌阅上是 v2.0 的另一个文件，用户 2026-09-28 选定）
    ["footer"] = { ["text_font_face"] = "./fonts/京華老宋体v3.0.ttf" },
    -- 阅读背景：再生纸纹理（koreader/backgrounds/，apply.sh 拷到设备的 backgrounds/；用户 2026-09-29 要）。KOReader 菜单里没有这个设置，
    -- 只能写这里；路径要写设备上的绝对路径，所以放在设备层。不想要：删掉这一行再 apply（或 --uninstall 撤方案）。
    ["cre_background_image"] = "/mnt/us/koreader/backgrounds/recycled-medium.png", -- 中档（用户 2026-09-29：浅的太淡）
        -- 亮度同步关掉（2026-10-02）：开机独占时 Kindle 自带的自动亮度不一定在跑，改用自己写的自动前光插件 autolight（见 koreader/README.md）；
    -- 这个插件只留色温同步。
    ["kindleautobrightness_enabled"] = false,
    ["autolight_enabled"] = true,
    ["plugins_disabled"] = { ["hotkeys"] = true }, -- Kindle PW12 没有实体键
    -- 色温同步：KOReader 读暖光时取 Kindle 按自己的时间表设的当前值（用户 2026-10-02 在设备上打开）。
    -- 别再开 KOReader 自带的自动色温（autowarmth_activate），两套时间表会来回改暖光（插件 README）。
    ["kindleautobrightness_warmth_enabled"] = true,
    -- 文件管理器起始目录：存储根下的 books（用户 2026-10-02 定，两台一样；apply.sh 按 device.conf 的 BOOKS_DIR 建好）
    ["home_dir"] = "/mnt/us/books",
}
