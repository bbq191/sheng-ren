-- 阅读进度同步（settings/kosync.lua 补丁，插件 kosync.koplugin 的设置，值都在 "settings" 表里）。
-- 键名和取值按 KOReader v2026.07 的 plugins/kosync.koplugin/main.lua 核过。
-- 账号（username/userkey）在设备上登录时由插件自己写，不进仓库（密码只在设备上输）。服务器用自己的 sync.vksight.com（官方 sync.koreader.rocks 2026-09-29 不响应；
-- 服务端程序和部署都在 vksight 仓库 ops/kosync/；2026-10-02 恢复，用户要两台同步）。
return {
    ["settings"] = {
        -- 按文件名认书，不按文件内容：优化规则一升级、书重新生成，文件字节就变，按内容（partial md5）认会把进度断开；
        -- 产物文件名只跟书名走，重新生成不变（CHECKSUM_METHOD.FILENAME = 1）
        ["custom_server"] = "https://sync.vksight.com",
        ["checksum_method"] = 1,
        ["auto_sync"] = true,
        -- 下面两项写出缺省值：设备上还没有 kosync.lua 时，只写上面两项会让"settings"表缺键，插件拿 nil 当策略就什么都不做
        ["sync_forward"] = 1,  -- 别的设备读得更靠后：问一下再跳（PROMPT）
        ["sync_backward"] = 3, -- 别的设备读得更靠前：不跳（DISABLE）
    },
}
