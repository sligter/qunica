//! The usage guide bundled into the binary.
//!
//! The Assistant answers "how do I…" questions from these pages rather than
//! from the model's own recollection, which would describe some other build of
//! some other product. They are embedded with `include_str!` so they ship with
//! the executable and cannot drift out of the install.
//!
//! Search is deliberately a keyword overlap rather than an embedding: it needs
//! no dependency, no index to build at startup, and no model call, and over a
//! dozen documents with distinct vocabularies it is enough.

/// One page of the guide.
#[derive(Debug, Clone, Copy)]
pub struct Doc {
    /// Stable identifier. Appears in tool output so the model can ask for one
    /// page by name.
    pub slug: &'static str,
    /// Human-readable title.
    pub title: &'static str,
    /// Extra search terms that do not appear as headings but are what a user
    /// would actually type. Without these, "api key" misses the providers page
    /// because the heading says "Fields".
    pub keywords: &'static str,
    /// The page itself.
    pub body: &'static str,
}

/// Largest `AppDocs` payload, in bytes.
///
/// The Assistant reads results in a small panel and pays for them in context,
/// so a broad query must not return the whole guide.
pub const MAX_DOCS_OUTPUT_BYTES: usize = 24_000;

/// Largest excerpt taken from any one document, in characters.
const MAX_EXCERPT_CHARS: usize = 4_000;

/// Most documents returned for one query.
const MAX_RESULTS: usize = 3;

macro_rules! doc {
    ($slug:literal, $title:literal, $keywords:literal) => {
        Doc {
            slug: $slug,
            title: $title,
            keywords: $keywords,
            body: include_str!(concat!("guide/", $slug, ".md")),
        }
    };
}

static DOCS: &[Doc] = &[
    doc!(
        "getting-started",
        "Getting started",
        "setup first run onboarding install begin new order steps concepts"
    ),
    doc!(
        "providers",
        "LLM providers",
        "provider api key model openai anthropic claude gemini base url token credentials \
         discover catalog default model"
    ),
    doc!(
        "workspaces",
        "Workspaces",
        "workspace folder directory local path files sandbox root mount share auto create"
    ),
    doc!(
        "agents",
        "Agents",
        "agent create tools prompt system prompt runtime model vision bash read write edit \
         deletefile glob grep websearch fetch group notes"
    ),
    doc!(
        "groups",
        "Groups",
        "group multi agent scheduler mention free speech proactive moderator topology mesh \
         star ring hierarchical budget tokens turn template reusable shared notes scratchpad"
    ),
    doc!(
        "direct-chats",
        "Direct chats",
        "direct chat one on one private conversation title reset context clear history"
    ),
    doc!(
        "skills",
        "Skills",
        "skill instructions import github package markdown frontmatter skillmanager mount"
    ),
    doc!(
        "mcp-servers",
        "MCP servers",
        "mcp model context protocol stdio http sse transport tool server headers session \
         connect test external tools"
    ),
    doc!(
        "external-cli-agents",
        "External CLI agents",
        "acp codex claude code cli external runtime install npm thinking effort permission \
         sandbox audit"
    ),
    doc!(
        "workspace-files",
        "Workspace files",
        "file panel attachment drag drop preview edit save conflict digest html pdf image \
         git diff commit branch stage"
    ),
    doc!(
        "terminal",
        "Terminal",
        "terminal shell pty tab console command prompt desktop ctrl backtick"
    ),
    doc!(
        "settings",
        "Settings",
        "settings appearance theme dark light language locale web search tavily logs data \
         directory sqlite tray"
    ),
    doc!(
        "assistant",
        "The built-in assistant",
        "assistant helper floating dock panel approve approval propose staged action \
         prefill history scratch temp shell"
    ),
];

/// Every bundled page.
pub fn all() -> &'static [Doc] {
    DOCS
}

/// Look up one page by slug.
pub fn by_slug(slug: &str) -> Option<&'static Doc> {
    let slug = slug.trim();
    DOCS.iter().find(|doc| doc.slug == slug)
}

/// The slug and title of every page, for offering an index.
pub fn index() -> Vec<(&'static str, &'static str)> {
    DOCS.iter().map(|doc| (doc.slug, doc.title)).collect()
}

/// A page matched by a query, with the part of it worth reading.
#[derive(Debug, Clone)]
pub struct DocMatch {
    pub slug: &'static str,
    pub title: &'static str,
    pub excerpt: String,
}

/// Find the pages most relevant to `query`.
///
/// Returns empty when nothing scores, rather than the least-bad page: a
/// confidently-presented wrong page is worse than "I don't have that", because
/// the model will summarize it as the answer.
pub fn search(query: &str) -> Vec<DocMatch> {
    let terms = terms_of(query);
    if terms.is_empty() {
        return Vec::new();
    }

    let mut scored: Vec<(u32, &'static Doc)> = DOCS
        .iter()
        .filter_map(|doc| {
            let score = score(doc, &terms);
            (score > 0).then_some((score, doc))
        })
        .collect();

    // Ties break by slug so the same query always returns the same order —
    // a result set that reshuffles per call defeats prompt caching.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.slug.cmp(b.1.slug)));
    scored.truncate(MAX_RESULTS);

    scored
        .into_iter()
        .map(|(_, doc)| DocMatch {
            slug: doc.slug,
            title: doc.title,
            excerpt: excerpt(doc, &terms),
        })
        .collect()
}

/// Split text into lowercase alphanumeric words, dropping noise.
fn terms_of(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(str::trim)
        .filter(|word| word.len() > 1)
        .map(str::to_lowercase)
        .filter(|word| !is_stop_word(word))
        .collect()
}

/// Words too common in questions about an app to discriminate between pages.
fn is_stop_word(word: &str) -> bool {
    matches!(
        word,
        "the"
            | "and"
            | "for"
            | "how"
            | "what"
            | "why"
            | "when"
            | "where"
            | "can"
            | "does"
            | "did"
            | "with"
            | "from"
            | "this"
            | "that"
            | "you"
            | "your"
            | "are"
            | "was"
            | "get"
            | "set"
            | "use"
            | "using"
            | "into"
            | "app"
            | "qunica"
    )
}

/// Weighted overlap between a query's terms and one document.
///
/// A term in the title is worth most, then a declared keyword, then a heading,
/// then body text. Body hits are capped so a page that merely mentions a word
/// many times cannot outrank the page actually about it.
fn score(doc: &Doc, terms: &[String]) -> u32 {
    let title = doc.title.to_lowercase();
    let keywords = doc.keywords.to_lowercase();
    let slug = doc.slug.to_lowercase();
    let body = doc.body.to_lowercase();
    let headings: String = doc
        .body
        .lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();

    let mut total = 0;
    for term in terms {
        if slug.contains(term.as_str()) {
            total += 10;
        }
        if title.contains(term.as_str()) {
            total += 8;
        }
        if keywords.split_whitespace().any(|word| word == term) {
            total += 6;
        }
        if headings.contains(term.as_str()) {
            total += 3;
        }
        if body.contains(term.as_str()) {
            total += 1;
        }
    }
    total
}

/// The part of a document worth returning.
///
/// Short pages come back whole. Long ones are cut at the section heading whose
/// following text matches best, so the excerpt starts at a heading rather than
/// mid-sentence.
fn excerpt(doc: &Doc, terms: &[String]) -> String {
    if doc.body.chars().count() <= MAX_EXCERPT_CHARS {
        return doc.body.to_string();
    }

    let sections = split_sections(doc.body);
    let best = sections
        .iter()
        .enumerate()
        .max_by_key(|(index, section)| {
            let lower = section.to_lowercase();
            let hits: usize = terms
                .iter()
                .filter(|term| lower.contains(term.as_str()))
                .count();
            // Prefer an earlier section on a tie: the opening of a page is
            // usually its definition.
            (hits, usize::MAX - index)
        })
        .map(|(index, _)| index)
        .unwrap_or(0);

    let mut out = String::new();
    // Always lead with the page's own title block so the excerpt is
    // self-describing even when it starts mid-page.
    if best > 0 {
        if let Some(first) = sections.first() {
            push_bounded(&mut out, first);
        }
    }
    for section in sections.iter().skip(best) {
        if out.chars().count() >= MAX_EXCERPT_CHARS {
            break;
        }
        push_bounded(&mut out, section);
    }
    out.trim_end().to_string()
}

fn push_bounded(out: &mut String, section: &str) {
    let remaining = MAX_EXCERPT_CHARS.saturating_sub(out.chars().count());
    if remaining == 0 {
        return;
    }
    for ch in section.chars().take(remaining) {
        out.push(ch);
    }
    out.push('\n');
}

/// Split a page at its `##` headings, keeping each heading with its body.
fn split_sections(body: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current = String::new();
    for line in body.lines() {
        if line.starts_with("## ") && !current.trim().is_empty() {
            sections.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        sections.push(current);
    }
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_is_reachable_by_its_own_title() {
        // A page its own title cannot find is a page the Assistant will never
        // surface.
        for doc in all() {
            let found = search(doc.title);
            assert!(
                found.iter().any(|hit| hit.slug == doc.slug),
                "{} is unreachable by its title {:?}",
                doc.slug,
                doc.title
            );
        }
    }

    #[test]
    fn common_questions_reach_the_right_page() {
        for (query, expected) in [
            ("how do I add an api key", "providers"),
            ("mcp stdio transport", "mcp-servers"),
            ("terminal shell", "terminal"),
            ("dark theme", "settings"),
            ("codex cli", "external-cli-agents"),
            ("drag a file into the composer", "workspace-files"),
            ("reusable group template", "groups"),
            ("shared group notes", "groups"),
        ] {
            let found = search(query);
            assert!(
                found.iter().any(|hit| hit.slug == expected),
                "{query:?} did not reach {expected}; got {:?}",
                found.iter().map(|hit| hit.slug).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn an_unrelated_query_matches_nothing() {
        assert!(search("kubernetes helm ingress").is_empty());
        assert!(search("").is_empty());
        // Stop words alone carry no signal.
        assert!(search("how does the app").is_empty());
    }

    #[test]
    fn excerpts_stay_within_their_bound() {
        for doc in all() {
            let text = excerpt(doc, &["workspace".to_string()]);
            assert!(
                text.chars().count() <= MAX_EXCERPT_CHARS + 1,
                "{} excerpt was {} chars",
                doc.slug,
                text.chars().count()
            );
        }
    }
}
