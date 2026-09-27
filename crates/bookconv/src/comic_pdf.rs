//! 页码分段书签：没有可用目录的漫画（PDF 裁白边产物、清洗层给漫画补的目录）按页分段，至少能按段跳转。

/// 没有源目录时的兜底书签：每 [`FALLBACK_TOC_PAGES`] 页一条，标题"第 N–M 页"（页码 1 起），如实标注不是章节。
const FALLBACK_TOC_PAGES: usize = 20;
pub(crate) fn page_chunk_titles(total_pages: usize) -> Vec<(usize, String)> {
    (0..total_pages)
        .step_by(FALLBACK_TOC_PAGES)
        .map(|start| (start, format!("第 {}–{} 页", start + 1, (start + FALLBACK_TOC_PAGES).min(total_pages))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_chunk_titles_boundaries() {
        assert_eq!(page_chunk_titles(1), vec![(0, "第 1–1 页".to_string())]);
        assert_eq!(page_chunk_titles(20).len(), 1);
        assert_eq!(page_chunk_titles(21).last(), Some(&(20, "第 21–21 页".to_string())));
    }
}
