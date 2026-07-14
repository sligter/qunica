use std::collections::HashSet;

pub struct MentionTarget<'a> {
    pub agent_id: &'a str,
    pub display_name: &'a str,
}

/// Finds agent mentions in visible Markdown prose, preserving textual order.
pub fn scan_visible_mentions(markdown: &str, candidates: &[MentionTarget<'_>]) -> Vec<String> {
    if candidates.is_empty() || !markdown.contains('@') {
        return Vec::new();
    }

    let visible = visible_markdown(markdown);
    let chars: Vec<char> = visible.chars().collect();
    let lower: Vec<char> = visible.to_lowercase().chars().collect();
    let mut names = candidates
        .iter()
        .filter(|candidate| !candidate.display_name.is_empty())
        .map(|candidate| {
            (
                candidate.agent_id,
                candidate
                    .display_name
                    .to_lowercase()
                    .chars()
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    names.sort_by_key(|(_, name)| std::cmp::Reverse(name.len()));

    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '@' || index > 0 && is_name_char(chars[index - 1]) {
            index += 1;
            continue;
        }
        let mut matched = false;
        for (agent_id, name) in &names {
            let end = index + 1 + name.len();
            if end > lower.len() || &lower[index + 1..end] != name.as_slice() {
                continue;
            }
            if end < chars.len() && is_name_char(chars[end]) {
                continue;
            }
            if seen.insert(*agent_id) {
                found.push((*agent_id).to_owned());
            }
            index = end;
            matched = true;
            break;
        }
        if !matched {
            index += 1;
        }
    }
    found
}

fn visible_markdown(markdown: &str) -> String {
    let mut output = String::with_capacity(markdown.len());
    let mut fence: Option<(char, usize)> = None;
    let mut inline_ticks = 0;
    let mut consumed = 0;

    for line in markdown.split_inclusive('\n') {
        consumed += line.len();
        let remaining = &markdown[consumed..];
        let content = line.strip_suffix('\n').unwrap_or(line);
        let trimmed = content.trim_start_matches(' ');
        let indent = content.len() - trimmed.len();

        if let Some((fence_char, fence_len)) = fence {
            if indent <= 3 && is_closing_fence(trimmed, fence_char, fence_len) {
                fence = None;
            }
            output.extend(std::iter::repeat_n(' ', content.chars().count()));
        } else if indent <= 3 && trimmed.starts_with('>') {
            output.extend(std::iter::repeat_n(' ', content.chars().count()));
        } else if indent <= 3 && opening_fence(trimmed).is_some() {
            fence = opening_fence(trimmed);
            output.extend(std::iter::repeat_n(' ', content.chars().count()));
        } else if inline_ticks == 0 && indent <= 3 && is_reference_definition(trimmed) {
            output.extend(std::iter::repeat_n(' ', content.chars().count()));
        } else {
            output.push_str(&mask_inline_and_destinations(
                content,
                remaining,
                &mut inline_ticks,
            ));
        }
        if line.ends_with('\n') {
            output.push('\n');
        }
    }
    output
}

fn is_reference_definition(line: &str) -> bool {
    let chars = line.chars().collect::<Vec<_>>();
    if chars.first() != Some(&'[') {
        return false;
    }
    let Some(label_end) =
        (1..chars.len()).find(|index| chars[*index] == ']' && !is_escaped(&chars, *index))
    else {
        return false;
    };
    if label_end == 1 {
        return false;
    }
    chars.get(label_end + 1) == Some(&':')
}

fn opening_fence(line: &str) -> Option<(char, usize)> {
    let marker = line.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let len = line.chars().take_while(|ch| *ch == marker).count();
    if len < 3 || marker == '`' && line.chars().skip(len).any(|ch| ch == '`') {
        return None;
    }
    Some((marker, len))
}

fn is_closing_fence(line: &str, marker: char, opening_len: usize) -> bool {
    if !line.starts_with(marker) {
        return false;
    }
    let len = line.chars().take_while(|ch| *ch == marker).count();
    len >= opening_len && line.chars().skip(len).all(char::is_whitespace)
}

fn mask_inline_and_destinations(
    line: &str,
    remaining_markdown: &str,
    inline_ticks: &mut usize,
) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    let mut output = chars.clone();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '`' && !is_escaped(&chars, index) {
            let run = chars[index..].iter().take_while(|ch| **ch == '`').count();
            if *inline_ticks == 0 {
                if !has_matching_tick_run(&chars[index + run..], remaining_markdown, run) {
                    index += run;
                    continue;
                }
                *inline_ticks = run;
            } else if run == *inline_ticks {
                *inline_ticks = 0;
            }
            output[index..index + run].fill(' ');
            index += run;
            continue;
        }
        if *inline_ticks > 0 {
            output[index] = ' ';
            index += 1;
            continue;
        }
        if chars[index] == ']' {
            let mut destination = index + 1;
            while destination < chars.len() && chars[destination].is_whitespace() {
                destination += 1;
            }
            if destination < chars.len()
                && chars[destination] == '('
                && !is_escaped(&chars, destination)
            {
                let mut depth = 0usize;
                let mut end = destination;
                while end < chars.len() {
                    match chars[end] {
                        '(' if !is_escaped(&chars, end) => depth += 1,
                        ')' if !is_escaped(&chars, end) => {
                            depth -= 1;
                            if depth == 0 {
                                end += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    end += 1;
                }
                output[destination..end].fill(' ');
                index = end;
                continue;
            }
        }
        index += 1;
    }
    output.into_iter().collect()
}

fn has_matching_tick_run(current_line: &[char], remaining_markdown: &str, expected: usize) -> bool {
    let chars = current_line
        .iter()
        .copied()
        .chain(remaining_markdown.chars())
        .collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '`' || is_escaped(&chars, index) {
            index += 1;
            continue;
        }
        let run = chars[index..].iter().take_while(|ch| **ch == '`').count();
        if run == expected {
            return true;
        }
        index += run;
    }
    false
}

fn is_escaped(chars: &[char], index: usize) -> bool {
    let slash_count = chars[..index]
        .iter()
        .rev()
        .take_while(|ch| **ch == '\\')
        .count();
    slash_count % 2 == 1
}

fn is_name_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '-' || ('\u{4e00}'..='\u{9fff}').contains(&ch)
}
