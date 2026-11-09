use std::fs;

use eqbench::build::{fix_class_name, fix_package};
use eqbench::case::{CaseId, Dataset, Kind, Version};
use eqbench::dataset_dir;

fn dataset() -> Dataset {
    Dataset::load(&dataset_dir()).expect("the dataset should load")
}

#[test]
fn discovers_every_java_case() {
    let dataset = dataset();
    let eq = dataset
        .cases
        .iter()
        .filter(|c| c.id.kind == Kind::Eq)
        .count();
    let neq = dataset
        .cases
        .iter()
        .filter(|c| c.id.kind == Kind::Neq)
        .count();
    // 272 pairs, without the 24 whole-program ones (12 Eq, 12 Neq).
    assert_eq!(dataset.cases.len(), 248);
    assert_eq!(eq, 135);
    assert_eq!(neq, 113);
    assert!(
        dataset
            .cases
            .iter()
            .all(|c| c.old.is_file() && c.new.is_file())
    );
}

#[test]
fn case_ids_are_unique_and_parse_back() {
    let dataset = dataset();
    let mut ids: Vec<String> = dataset.cases.iter().map(|c| c.id.to_string()).collect();
    for id in &ids {
        let parsed: CaseId = id.parse().unwrap();
        assert!(dataset.find(&parsed).is_some(), "{id} should be found");
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), dataset.cases.len());
}

#[test]
fn parses_every_description() {
    let dataset = dataset();
    for case in &dataset.cases {
        assert!(!case.desc.method.is_empty(), "{}", case.id);
        assert!(!case.desc.changes.is_empty(), "{}", case.id);
        // Only non-equivalent cases come with a counter-example.
        if case.desc.counter_example.is_some() {
            assert_eq!(case.id.kind, Kind::Neq, "{}", case.id);
        }
    }
}

#[test]
fn staged_sources_declare_their_own_package_and_class() {
    let dataset = dataset();
    let mut fixed = 0;
    for case in &dataset.cases {
        for version in Version::ALL {
            let path = case.source(version);
            let class = path.file_stem().unwrap().to_str().unwrap();
            let other = if class == "oldV" { "newV" } else { "oldV" };
            let source = fs::read_to_string(path).unwrap();

            let (source, package_fixed) = fix_package(&source, &case.id.java_package());
            let (source, class_fixed) = fix_class_name(&source, class);
            fixed += usize::from(package_fixed || class_fixed);

            let package = format!("package {};", case.id.java_package());
            assert!(source.contains(&package), "{} ({version})", case.id);
            assert!(
                !source.contains(&format!("public class {other}")),
                "{} ({version})",
                case.id
            );
        }
    }
    // 10 wrong packages and 1 wrong class name in the dataset.
    assert_eq!(fixed, 11);
}
