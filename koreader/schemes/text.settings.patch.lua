-- 文字书方案（settings.reader.lua 补丁）。全局设置本身就是文字书方案：任何没有单书设置的书都用它。
-- 排版（行距、字重、字体……）照个人设置（personal/）不动，这里只加功能项（用户 2026-09-28 选定）。
-- 键名全部按设备上 KOReader v2026.07.2 的源码核过（docs/koreader.md「键名怎么核」）。
return {
    -- ── 脚注回得去 ──
    ["footnote_link_in_popup"] = true,          -- 点脚注在底部弹窗显示，不跳页
    ["link_prefer_footnote"] = true,            -- 中文书的脚注常没有 epub:type，放宽"是不是脚注"的判定
    ["larger_tap_area_to_follow_links"] = true, -- 上标注释号太小，放大点击命中区
    ["swipe_to_go_back"] = true,                -- 跟着链接跳过去之后，左→右滑回原处（没有跳转记录时就是上一页，翻页不受影响）
    ["footnote_popup_relative_font_size"] = -2, -- 弹窗字号比正文小 2（正文 17 → 15，约小一号；用户 2026-09-29：注释比正文小 1 号）

    -- ── 分页与注释：交给优化器的产物 ──
    -- 优化器靠"拆文件"分页（章标题独立一页、节与节分页，见 docs/typesetting.md），KOReader 在每个文件（DocFragment）开头换页。
    -- 下面几条样式调整会和它打架，撤掉（style_tweaks 里只存开着的项，删掉 = 关）：
    ["style_tweaks"] = {
        -- "避免章末空白页"：DocFragment { page-break-before: auto }，会把拆开的文件又连成一片——节与节不分页的根因
        ["docfragment_page-break-before_avoid "] = "__DELETE__", -- 键名末尾的空格是 KOReader 源码里就有的
        -- "H1/H2/H3 前换页"：副标题会和章标题分到两页（优化器让副标题留在章标题页）
        ["h1_page-break-before_always"] = "__DELETE__",
        ["h2_page-break-before_always"] = "__DELETE__",
        ["h3_page-break-before_always"] = "__DELETE__",
        -- "页内注释"：把注释排到引用它的那页页底；我们要点开弹窗（footnote_link_in_popup），注释留在章末
        ["footnote-inpage_epub"] = "__DELETE__",
        ["inpage_footnote_font-size_smaller"] = "__DELETE__",
    },

    -- ── 阅读进度同步（掌阅、Kindle 读同一份产物；插件设置在 settings/kosync.lua，见 schemes/kosync.patch.lua） ──
    -- 自动同步要能自己开 Wi-Fi：Kindle 上这项不是 turn_on 时，KOReader 启动时会把自动同步关掉（kosync main.lua）。
    -- Android（掌阅）不受这项影响。
    ["wifi_enable_action"] = "turn_on",

    -- ── 断行 ──
    ["text_lang_fallback"] = "zh-CN",           -- 书没标语言时按中文断行（缺省 en-US）

    -- ── 刷新 ──
    ["full_refresh_count"] = 16,                -- 文字页每 16 页全刷一次清残影（缺省 6）。带图片的页 KOReader 缺省就每页全刷
                                                -- （refresh_on_pages_with_images 缺省开），漫画不用另设
}
