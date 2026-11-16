//! 行内收集：一个文档的解析状态 [`Doc`]、还开着的样式区间 [`OpenRun`] 和行内缓冲 [`Inline`]。

use super::*;

pub(in crate::write) struct Doc<'a> {
    pub(in crate::write) path: &'a str,
    pub(in crate::write) sheet: Sheet,
    pub(in crate::write) lang: Option<String>,
    /// 还没落到内容上的锚点：没有文字的元素（`<span id="x"></span>`、`<div id="x"></div>`）的 id。挂到文档顺序里
    /// 下一个生成的块的开头（[`Doc::take_pending`]）。以前直接丢掉，指向它们的链接、目录项退回到文件开头
    /// （《福尔摩斯探案全集》目录页「第一册」指向页末插图前的空锚点，点了停在目录页开头；全书 1102 处）。
    pub(in crate::write) pending: std::cell::RefCell<Vec<String>>,
    /// 还没落到块上的空段折出来的高度（根 em，见 [`BLANK_LINE_FOLD`]）：和 `pending` 一样给下一个生成的块。
    pub(in crate::write) gap: std::cell::RefCell<Gap>,
    /// 包着当前位置、还没走完的行内链接（`<a href>`）的目标，外层在前：里面的块级元素（`<a href><p>…</p></a>`）另起行内缓冲时
    /// 接着开（[`Doc::inline`]）。以前链接只在 `<a>` 自己的缓冲里，里面块的文字没有链接（写出器 15）。
    pub(in crate::write) links: std::cell::RefCell<Vec<(String, String)>>,
}

/// 一个还开着的行内区间（[`Inline::open`]）。
#[derive(Clone)]
pub(super) struct OpenRun {
    start: usize,
    /// 插到 `runs` 的哪个下标（外层的区间排在里层的前面）。
    run_at: usize,
    run: Run,
    /// 开的时候前面有个还没写出的空格（`foo <a>bar</a>`）：这个空格写出来时正好落在起点上，起点后挪一格，空格不算区间里的
    /// （以前算进去：offset 3 len 4，写出器 15）。元素的 id 同样后挪（[`Inline::push_id`]），注释配对按区间起点找 id，两边要一致。
    after_space: bool,
}

/// 收集行内内容：文字（空白按 CSS 折叠）、`<br>`、行内元素的样式区间、遇到图片就切开。
pub(in crate::write) struct Inline {
    pub(super) text: String,
    chars: usize,
    pub(super) runs: Vec<Run>,
    pub(super) ids: Vec<(String, usize)>,
    /// `ids` 里登记时前面有个还没写出的空格的（同 [`OpenRun::after_space`]：空格写出来时锚点后挪一格，落在元素的第一个字上）。
    space_ids: Vec<usize>,
    pending_space: bool,
    /// 还没走完的行内元素的区间（外层在前，长度待定）。
    /// 走到一半遇到块或图片要切开（[`Doc::flush`]）时，区间在切开处截断，到新缓冲从 0 接着开（见 [`Inline::cut_open`]）。
    pub(super) open: Vec<OpenRun>,
    pub(super) images: Vec<InlineImage>,
}

impl Inline {
    pub(super) fn new() -> Self {
        Inline {
            text: String::new(),
            chars: 0,
            runs: Vec::new(),
            ids: Vec::new(),
            space_ids: Vec::new(),
            pending_space: false,
            open: Vec::new(),
            images: Vec::new(),
        }
    }

    /// 行内元素的 id：锚点在它的第一个字上。
    pub(super) fn push_id(&mut self, id: &str) {
        if self.pending_space {
            self.space_ids.push(self.ids.len());
        }
        self.ids.push((id.to_string(), self.chars));
    }

    /// 开一个行内区间（行内元素进来时）。
    pub(super) fn open_run(&mut self, run: Run) {
        self.open.push(OpenRun {
            start: self.chars,
            run_at: self.runs.len(),
            run,
            after_space: self.pending_space,
        });
    }

    /// 一个行内元素走完（或在切开处截断）：有字的话把区间插进 `runs`。
    pub(super) fn close_run(&mut self, o: OpenRun) {
        if self.chars > o.start {
            self.runs.insert(
                o.run_at,
                Run {
                    start: o.start,
                    len: self.chars - o.start,
                    ..o.run
                },
            );
        }
    }

    /// 切开之前：还开着的区间（从里层到外层，外层插在前面）截断在这里；返回给新缓冲用的、从 0 重新开始的区间。
    pub(super) fn cut_open(&mut self) -> Vec<OpenRun> {
        let open = std::mem::take(&mut self.open);
        for o in open.iter().rev() {
            self.close_run(o.clone());
        }
        open.into_iter()
            .map(|o| OpenRun {
                start: 0,
                run_at: 0,
                run: o.run,
                after_space: false,
            })
            .collect()
    }

    pub(super) fn push_text(&mut self, s: &str, pre: bool) {
        for c in s.chars() {
            if !pre && c.is_ascii_whitespace() {
                self.pending_space = self.chars > 0 && !self.text.ends_with('\n');
                continue;
            }
            self.push_char(c);
        }
    }

    /// 行内图片：文字里占一个 [`OBJ`]。
    pub(super) fn push_image(&mut self, mut img: InlineImage) {
        self.push_char(OBJ);
        img.at = self.chars - 1;
        self.images.push(img);
    }

    /// 写出一个非空白字符（前面有待写的空格先写空格）。
    pub(super) fn push_char(&mut self, c: char) {
        if self.pending_space {
            for o in self
                .open
                .iter_mut()
                .filter(|o| o.after_space && o.start == self.chars)
            {
                o.start += 1;
            }
            for i in self.space_ids.drain(..) {
                if self.ids[i].1 == self.chars {
                    self.ids[i].1 += 1;
                }
            }
            self.text.push(' ');
            self.chars += 1;
            self.pending_space = false;
        }
        self.text.push(c);
        self.chars += 1;
    }

    pub(super) fn push_break(&mut self) {
        self.pending_space = false;
        self.space_ids.clear();
        self.text.push('\n');
        self.chars += 1;
    }

    /// 没有字（空白和行内图片不算字）。
    pub(super) fn is_empty(&self) -> bool {
        self.text.chars().all(|c| c.is_whitespace() || c == OBJ)
    }

    pub(super) fn take(&mut self) -> Inline {
        std::mem::replace(self, Inline::new())
    }

    /// 字符 `from..to` 这一段（两头是独立成块的图片，见 [`Doc::flush`]）：区间、锚点、行内图片挪到这一段的偏移。
    /// 紧挨图片的空格去掉（折叠空白时图片两边补出来的，以前图片切开文字，这两个空格不会写出来）；`to` 处的锚点归这一段（落在末尾）。
    pub(super) fn slice(&self, chars: &[char], from: usize, to: usize) -> Inline {
        let lead = usize::from(from > 0 && chars.get(from) == Some(&' ') && from < to);
        let trail = usize::from(to < chars.len() && to > from + lead && chars[to - 1] == ' ');
        let (a, z) = (from + lead, to - trail);
        let mut out = Inline::new();
        out.text = chars[a..z].iter().collect();
        out.chars = z - a;
        out.runs = self
            .runs
            .iter()
            .filter_map(|r| {
                let (s, e) = (r.start.max(a), (r.start + r.len).min(z));
                (e > s).then(|| Run {
                    start: s - a,
                    len: e - s,
                    ..r.clone()
                })
            })
            .collect();
        let after = |o: usize| if from == 0 { true } else { o >= from };
        out.ids = self
            .ids
            .iter()
            .filter(|(_, o)| after(*o) && *o <= to)
            .map(|(i, o)| (i.clone(), o.saturating_sub(a).min(out.chars)))
            .collect();
        out.images = self
            .images
            .iter()
            .filter(|im| im.at >= a && im.at < z)
            .map(|im| InlineImage {
                at: im.at - a,
                ..im.clone()
            })
            .collect();
        out
    }
}
