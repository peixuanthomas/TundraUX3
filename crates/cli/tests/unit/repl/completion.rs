use super::*;

#[test]
fn large_candidate_lists_do_not_delay_each_typed_character() {
    let commands = system_candidates((0..100_000).map(|i| format!("command-{i:056}")).collect());
    let typo = "z".repeat(64);
    let pasted = "x".repeat(100_000);
    let start = Instant::now();
    for _ in 0..20 {
        let hint = command_hint("command-", 8, &commands).unwrap();
        assert!(hint.display().contains("Tab for more"));
        assert!(command_hint(&typo, typo.len(), &commands).is_none());
        assert!(command_hint(&pasted, pasted.len(), &commands).is_none());
    }
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "large PATH stalled hints: {:?}",
        start.elapsed()
    );
}

#[test]
fn bounded_typo_distance_preserves_insert_delete_replace_and_swap() {
    // Compare every short word pair against the unrestricted definition. The
    // production code may stop at 3, since only one/two-edit corrections show.
    fn reference(left: &[char], right: &[char]) -> usize {
        let mut table = vec![vec![0; right.len() + 1]; left.len() + 1];
        for (i, row) in table.iter_mut().enumerate() {
            row[0] = i;
        }
        for j in 0..=right.len() {
            table[0][j] = j;
        }
        for i in 1..=left.len() {
            for j in 1..=right.len() {
                table[i][j] = (table[i - 1][j] + 1)
                    .min(table[i][j - 1] + 1)
                    .min(table[i - 1][j - 1] + usize::from(left[i - 1] != right[j - 1]));
                if i > 1 && j > 1 && left[i - 1] == right[j - 2] && left[i - 2] == right[j - 1] {
                    table[i][j] = table[i][j].min(table[i - 2][j - 2] + 1);
                }
            }
        }
        table[left.len()][right.len()]
    }
    let words = (0..=5)
        .flat_map(|len| {
            (0..1 << len).map(move |bits| {
                (0..len)
                    .map(|bit| if bits & (1 << bit) == 0 { 'a' } else { '中' })
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    for left in &words {
        for right in &words {
            assert_eq!(
                edit_distance(left, right),
                reference(left, right).min(3),
                "{left:?}, {right:?}"
            );
        }
    }
}
