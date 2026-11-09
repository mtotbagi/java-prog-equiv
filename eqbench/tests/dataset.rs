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
    assert_eq!(dataset.cases.len(), 272);
    assert_eq!(eq, 147);
    assert_eq!(neq, 125);
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
    let with_desc: Vec<_> = dataset
        .cases
        .iter()
        .filter_map(|c| c.desc.as_ref())
        .collect();
    assert_eq!(with_desc.len(), 248);
    assert!(with_desc.iter().all(|d| !d.changes.is_empty()));

    // Only non-equivalent cases come with a counter-example.
    for case in &dataset.cases {
        if let Some(desc) = &case.desc
            && desc.counter_example.is_some()
        {
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
    // 12 wrong packages and 1 wrong class name in the dataset.
    assert_eq!(fixed, 13);
}
