//! "PDF 转出的 EPUB"来源标记：`book_id` 前缀约定。
/// PDF 转出的 EPUB 的 `BookMeta.book_id` 前缀（来源标记）。塞进 `publisher` 会污染真实元数据、往正文塞不可见
/// 占位不优雅，所以约定 `book_id` 前缀。
pub(super) const PDF_BOOK_ID_PREFIX: &str = "pdf:";
