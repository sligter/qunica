// Note: 正文是共享事实来源，规则随产品分发 — 见 .agents/notes/implemented/feature/2026-09-12-built-in-group-note-method.md。
pub(crate) const AUTHORING_GUIDE: &str = "Shared group note method (adapted from write-notes-like-deepseek):
Read relevant existing notes before making consequential decisions; update the existing note in place instead of appending a conversation log or duplicating it. Keep one durable topic per note, about 200 words.
Use this Markdown header: a # title on line 1, Status: proposed | implemented | rejected — <reason> | archived on line 2 (choose one), Since: yyyy-mm-dd on line 3, and Category: 决策 | 约定 | 踩坑 on line 4 (choose one). Preserve Since when editing.
Use exactly these four sections: ## Problem, ## Decision, ## Alternatives considered, ## Consequences. State each alternative's strongest case before explaining its rejection, including doing nothing / reusing the existing solution. Record benefits, costs and a concrete signal requiring reconsideration in Consequences.
New suggestions start as proposed. A single Agent's suggestion is not group consensus; agreement is not implementation. Record confirmation or implementation evidence in Decision. Use implemented only for verified present facts, without future plans or conversation residue. Give a reason for rejected; archive superseded guidance.
Preserve legacy free-form notes unless their conversion is requested. For code decisions already maintained in repository decision notes, link to that source instead of duplicating its body. The host owns filenames and index.md: never rename note files to encode status or manually edit the index.";

pub(crate) fn default_content(title: &str, since: &str) -> String {
    format!(
        "# {title}\nStatus: proposed\nSince: {since}\nCategory: 决策\n\n\
         ## Problem\n\nDescribe the problem and constraints.\n\n\
         ## Decision\n\nDescribe the proposal and its confirmation or implementation evidence.\n\n\
         ## Alternatives considered\n\n- Do nothing / reuse the existing solution: strongest case, then why it is insufficient.\n\n\
         ## Consequences\n\nBenefits, costs, and a concrete signal requiring reconsideration.\n"
    )
}
