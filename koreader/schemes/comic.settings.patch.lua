-- 漫画方案的自动切换（settings.reader.lua 补丁；配置档本体见 profiles.patch.lua）。
--
-- 怎么认出漫画：优化器给漫画的 OPF 打上 `<dc:subject>漫画</dc:subject>`（bookconv::comic_detect::COMIC_SUBJECT），
-- KOReader 把 dc:subject 读成书的 keywords，配置档按"书的元数据包含 漫画"触发。书放在设备上哪个目录都行，
-- 不用像按文件夹那样把漫画和小说分开放。没经过优化器的漫画没有这个标签，不会触发。
--
-- 打开漫画：「漫画·首次」只在这本书第一次打开时执行（is_new），设从右往左、去页边距等单书设置——之后在书里改了
--           （比如国漫/美漫关掉反向翻页）会记住，不会每次打开又被改回去；「漫画」每次打开都执行，隐藏状态栏。
-- 关闭漫画：执行「文字」，状态栏恢复文字书的样子。
-- 已知限制：漫画读到一半 KOReader 崩了没走到"关书"，状态栏会一直是隐藏的，打开/关闭任意一本漫画就恢复。
return {
    ["profiles_autoexec"] = {
        ["ReaderReadyAll"] = {
            ["漫画·首次"] = { ["doc_props"] = { ["keywords"] = "漫画" }, ["is_new"] = true },
            ["漫画"] = { ["doc_props"] = { ["keywords"] = "漫画" } },
        },
        ["CloseDocumentAll"] = {
            ["文字"] = { ["doc_props"] = { ["keywords"] = "漫画" } },
        },
    },
    -- 配置档靠这两个插件；个人设置里没停用它们，这里再确保一次
    ["plugins_disabled"] = {
        ["profiles"] = "__DELETE__",
        ["statistics"] = "__DELETE__", -- 状态栏"本章/全书剩余时间"靠它
    },
}
