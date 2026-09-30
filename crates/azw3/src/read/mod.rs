//! KF8（AZW3）读取器：**只给写出器回读自检和测试用**（`tests/roundtrip.rs`、书库的流程测试），不是输入格式——
//! 入库只收 EPUB、CBZ，遇到 AZW3 照样拒收。和写出器一样 clean-room：依 MobileRead 的 MOBI 容器文档 + 对 KF8 样本的
//! 黑盒分析实现，不看 KindleUnpack / Calibre 的代码。
//! - [`palm`]：PalmDB 容器、PalmDOC 解压、尾随字节、EXTH、INDX（片段/目录索引）；base32、正向变长整数与写出器共用。
//! - [`kf8`]：AZW3 → 可读的 EPUB（按目录位置切章，还原 `kindle:embed`/`kindle:pos` 引用），用来核对写出的内容读得回来。

pub mod kf8;
pub mod palm;
