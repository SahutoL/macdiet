use std::io::{self, BufRead, Write};

use anyhow::{Result, anyhow};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Selection {
    All,
    None,
    Indices(Vec<usize>),
}

pub(crate) fn prompt_action_selection(action_count: usize) -> Result<Selection> {
    let mut stderr = io::stderr().lock();
    write!(
        stderr,
        "実行するアクションを選択してください（例: 1,3-5 / all / none）[既定: all]: "
    )?;
    stderr.flush()?;

    let mut input = String::new();
    let mut stdin = io::stdin().lock();
    let n = stdin.read_line(&mut input)?;
    if n == 0 {
        return Ok(Selection::None);
    }
    parse_selection(&input, action_count)
}

pub(crate) fn parse_selection(input: &str, max: usize) -> Result<Selection> {
    if max == 0 {
        return Ok(Selection::None);
    }

    let s = input.trim();
    if s.is_empty() {
        return Ok(Selection::All);
    }

    let s = s.to_ascii_lowercase();
    match s.as_str() {
        "all" | "*" | "全部" | "全て" | "すべて" | "ぜんぶ" => return Ok(Selection::All),
        "none" | "no" | "n" | "q" | "quit" | "なし" | "無し" | "キャンセル" | "中止" => {
            return Ok(Selection::None);
        }
        _ => {}
    }

    let mut selected = vec![false; max];
    for token in s.split(|c: char| c == ',' || c.is_whitespace()) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }

        if let Some((start, end)) = token.split_once('-') {
            let start = start
                .trim()
                .parse::<usize>()
                .map_err(|_| anyhow!("範囲指定の開始が不正です: {token}"))?;
            let end = end
                .trim()
                .parse::<usize>()
                .map_err(|_| anyhow!("範囲指定の終了が不正です: {token}"))?;
            if start == 0 || end == 0 {
                return Err(anyhow!("選択は1始まりです（0は指定できません）: {token}"));
            }
            if start > end {
                return Err(anyhow!("範囲指定が不正です（start > end）: {token}"));
            }
            if end > max {
                return Err(anyhow!("選択が範囲外です（最大 {max}）: {token}"));
            }
            for i in start..=end {
                selected[i - 1] = true;
            }
        } else {
            let idx = token
                .parse::<usize>()
                .map_err(|_| anyhow!("選択が不正です: {token}"))?;
            if idx == 0 {
                return Err(anyhow!("選択は1始まりです（0は指定できません）: {token}"));
            }
            if idx > max {
                return Err(anyhow!("選択が範囲外です（最大 {max}）: {token}"));
            }
            selected[idx - 1] = true;
        }
    }

    let indices: Vec<usize> = selected
        .into_iter()
        .enumerate()
        .filter_map(|(idx, on)| on.then_some(idx))
        .collect();

    if indices.is_empty() {
        return Err(anyhow!(
            "アクションが選択されていません（'all' または 'none' を使用できます）"
        ));
    }

    Ok(Selection::Indices(indices))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_selection_empty_is_all() {
        assert_eq!(parse_selection("", 3).unwrap(), Selection::All);
        assert_eq!(parse_selection("   ", 3).unwrap(), Selection::All);
    }

    #[test]
    fn parse_selection_all_variants() {
        assert_eq!(parse_selection("all", 2).unwrap(), Selection::All);
        assert_eq!(parse_selection("*", 2).unwrap(), Selection::All);
    }

    #[test]
    fn parse_selection_none_variants() {
        for s in ["none", "no", "n", "q", "quit"] {
            assert_eq!(parse_selection(s, 2).unwrap(), Selection::None);
        }
    }

    #[test]
    fn parse_selection_numbers_and_ranges() {
        assert_eq!(
            parse_selection("1,3-4", 5).unwrap(),
            Selection::Indices(vec![0, 2, 3])
        );
        assert_eq!(
            parse_selection("2 5", 5).unwrap(),
            Selection::Indices(vec![1, 4])
        );
    }

    #[test]
    fn parse_selection_rejects_out_of_range() {
        assert!(parse_selection("3", 2).is_err());
        assert!(parse_selection("1-3", 2).is_err());
    }
}
