//! Turns a GitHub release body (release-please markdown) into the plain
//! heading and bullet lines the What's New dialog shows.

/// One rendered line: a section heading or a bullet under it.
#[derive(Debug, PartialEq)]
pub struct Line {
    pub heading: bool,
    pub text: String,
}

pub fn parse(body: &str) -> Vec<Line> {
    body.lines()
        .filter_map(|raw| {
            let l = raw.trim();
            if let Some(h) = l.strip_prefix("### ") {
                Some(Line {
                    heading: true,
                    text: clean(h),
                })
            } else {
                l.strip_prefix("* ")
                    .or_else(|| l.strip_prefix("- "))
                    .map(|b| Line {
                        heading: false,
                        text: clean(b),
                    })
            }
        })
        .filter(|l| !l.text.is_empty())
        .collect()
}

/// Drops release-please's `**scope:**` prefix and its trailing
/// ` ([#12](…)) ([abc1234](…))` link groups, then unwraps any other
/// `[text](url)` to its text.
fn clean(s: &str) -> String {
    let mut s = s.trim();
    if let Some(rest) = s.strip_prefix("**") {
        if let Some(i) = rest.find(":**") {
            s = rest[i + 3..].trim_start();
        }
    }
    while s.ends_with("))") {
        match s.rfind(" ([") {
            Some(i) => s = &s[..i],
            None => break,
        }
    }
    unlink(s).replace("**", "")
}

/// One pass: keeps a link's `[text]`, drops the `(url)` right after it.
/// ponytail: every `[`/`]` goes, even outside a link — fine for notes text.
fn unlink(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_url = false;
    let mut after_bracket = false;
    for c in s.chars() {
        match c {
            ')' if in_url => in_url = false,
            _ if in_url => {}
            '(' if after_bracket => in_url = true,
            '[' => {}
            ']' => {
                after_bracket = true;
                continue;
            }
            _ => out.push(c),
        }
        after_bracket = false;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_please_body_becomes_headings_and_bullets() {
        let body = "## [0.46.0](https://example.com/compare) (2026-09-12)\n\n\
            ### App Features\n\n\
            * **app:** show open connections as a rail ([#12](https://example.com/pull/12)) ([abc1234](https://example.com/commit/abc1234))\n\
            * add [docs](https://example.com/docs) link\n\n\
            ### Bug Fixes\n\n\
            - **app:** stop tab titles truncating ([def5678](https://example.com/commit/def5678))\n";
        let lines = parse(body);
        let got: Vec<(bool, &str)> = lines.iter().map(|l| (l.heading, l.text.as_str())).collect();
        assert_eq!(
            got,
            vec![
                (true, "App Features"),
                (false, "show open connections as a rail"),
                (false, "add docs link"),
                (true, "Bug Fixes"),
                (false, "stop tab titles truncating"),
            ]
        );
    }
}
