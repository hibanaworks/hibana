use super::{Protocol, message};
use serde_json::{Value, json};

#[test]
fn nested_visits_choices_and_poll_orders_keep_their_script_coordinates() {
    let mut label = 1;
    let source = Protocol::Seq(
        Box::new(Protocol::Roll(Box::new(Protocol::Route(
            Box::new(message(&mut label, 0, 1, false)),
            Box::new(message(&mut label, 0, 1, false)),
        )))),
        Box::new(Protocol::Par(
            Box::new(Protocol::Resolved(
                Box::new(message(&mut label, 1, 2, false)),
                Box::new(message(&mut label, 1, 2, false)),
                0,
            )),
            Box::new(Protocol::Seq(
                Box::new(message(&mut label, 2, 0, false)),
                Box::new(message(&mut label, 0, 2, false)),
            )),
        )),
    );
    let mut expected = Vec::new();
    for first in [1, 2] {
        for second in [1, 2] {
            for third in [3, 4] {
                for reverse in [false, true] {
                    expected.push(json!([
                        1,
                        [
                            4,
                            [3, [0, first, 0], first == 2],
                            [3, [0, second, 1], second == 2]
                        ],
                        [
                            2,
                            [3, [0, third, 2], third == 4],
                            [1, [0, 5, 3], [0, 6, 4]],
                            reverse
                        ]
                    ]));
                }
            }
        }
    }
    let mut scripts = Vec::new();
    source.write_scripts(&mut scripts).unwrap();
    let actual = String::from_utf8(scripts)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(source.execution_count(), 16);
    assert_eq!(actual, expected);
    assert_eq!(
        actual
            .iter()
            .map(Value::to_string)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        16
    );
}

#[test]
fn a_large_execution_space_can_be_indexed_without_materializing_earlier_trees() {
    let mut label = 0;
    let mut source = Protocol::Par(
        Box::new(message(&mut label, 0, 1, false)),
        Box::new(message(&mut label, 1, 0, false)),
    );
    for _ in 0..5 {
        source = Protocol::Roll(Box::new(source));
    }
    let count = 1usize.checked_shl(32).unwrap();
    let mut executions = source.executions();
    assert_eq!(executions.len(), count);
    let first = executions.next().unwrap().trace().line();
    assert_eq!(first, format!("{}\n", vec!["0,1"; 32].join(",")));
    assert_eq!(executions.len(), count - 1);
    let last = source.execution_at(count - 1).trace().line();
    assert_eq!(last, format!("{}\n", vec!["1,0"; 32].join(",")));
    assert_eq!(executions.take(2).count(), 2);
}

#[test]
fn script_output_errors_are_reported() {
    struct FailedOutput;
    impl std::io::Write for FailedOutput {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from(std::io::ErrorKind::StorageFull))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let source = message(&mut 0, 0, 1, false);
    assert_eq!(
        source.write_scripts(FailedOutput).unwrap_err().kind(),
        std::io::ErrorKind::StorageFull
    );
}
