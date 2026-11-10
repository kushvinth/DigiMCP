#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, name: &str, code: &str) -> ClassRow {
        ClassRow {
            id,
            label: name.to_string(),
            course_name: name.to_string(),
            course_code: code.to_string(),
            kind: "Lecture".into(),
            credits: json!(4),
            faculty: String::new(),
        }
    }

    #[test]
    fn resolves_classes_by_id_code_name_and_tokens() {
        let classes = vec![
            row(11, "Data Structures", "25CS201"),
            row(12, "Engineering Physics", "25PH101"),
        ];
        assert_eq!(pick_class(&classes, "11").unwrap().id, 11);
        assert_eq!(pick_class(&classes, "25ph101").unwrap().id, 12);
        assert_eq!(pick_class(&classes, "physics").unwrap().id, 12);
        assert_eq!(pick_class(&classes, "structures").unwrap().id, 11);
        assert_eq!(pick_class(&classes, "engineering   physics").unwrap().id, 12);
        assert!(pick_class(&classes, "chemistry").is_none());
        assert!(pick_class(&classes, "  ").is_none());
    }

    #[test]
    fn labels_survive_missing_fields() {
        let v = json!({"courseName":"Data\u{a0}Structures ","courseCode":"25CS201"});
        assert_eq!(class_label(&v), "Data Structures");
        let v = json!({"courseName":"Physics","courseComponentTypeName":"Practical"});
        assert_eq!(class_label(&v), "Physics (Practical)");
    }

    #[test]
    fn attendance_slack_is_conservative() {
        // Exactly on the line: no room to miss a class.
        assert_eq!(slack(30.0, 40.0, 75.0), (0.0, 0.0));
        // 9 of 10 is 90%: two more absences keep us at 75% (9 of 12).
        assert_eq!(slack(9.0, 10.0, 75.0), (2.0, 0.0));
        // 20 of 40 is 50%: 40 straight classes to reach 75% (60 of 80).
        assert_eq!(slack(20.0, 40.0, 75.0), (0.0, 40.0));
        // No classes held yet is not a failure.
        assert_eq!(slack(0.0, 0.0, 75.0), (0.0, 0.0));
    }

    #[test]
    fn unix_timestamps_render_for_agents() {
        assert!(unix_to_rfc3339(1_800_000_000).unwrap().starts_with("2027-"));
    }
}
