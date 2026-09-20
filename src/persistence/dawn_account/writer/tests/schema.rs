use super::*;

#[test]
fn unknown_graph_extensions_are_refused_without_changing_database_or_document() {
    for sql in [
        "ALTER TABLE characters ADD COLUMN extension TEXT DEFAULT 'default'; UPDATE characters SET extension='keep'",
        "ALTER TABLE profile_items ADD COLUMN extension BLOB; UPDATE profile_items SET extension=X'1234'",
        "ALTER TABLE item_rolls ADD COLUMN extension INTEGER",
        "CREATE TABLE extra(instance_soid TEXT REFERENCES character_items(instance_soid) ON DELETE CASCADE, value TEXT); INSERT INTO extra VALUES('4000000000000004','keep')",
        "CREATE TABLE extra(character_soid TEXT REFERENCES characters(soid) ON DELETE SET NULL); INSERT INTO extra VALUES('9EAA300100100101')",
        "CREATE TABLE extra(position INTEGER REFERENCES profile_items ON DELETE CASCADE); INSERT INTO extra VALUES(0)",
        "CREATE TRIGGER graph_change AFTER DELETE ON character_items BEGIN DELETE FROM missions; END",
        "PRAGMA user_version=6",
    ] {
        let (_temp, mut doc) = fixture();
        let db = Connection::open(&doc.path).unwrap();
        db.execute_batch(sql).unwrap();
        edit_settings(&mut doc);
        let original = doc.clone();
        let before = snapshot::capture(&db).unwrap();
        let error = save(&mut doc).unwrap_err().to_string();
        assert!(error.contains("layout is not supported"), "{sql}: {error}");
        assert_eq!(snapshot::capture(&db).unwrap(), before, "{sql}");
        assert_eq!(doc, original, "{sql}");
    }
}

#[test]
fn unrelated_extensions_remain_untouched() {
    let (_temp, mut doc) = fixture();
    let db = Connection::open(&doc.path).unwrap();
    db.execute_batch(
        "CREATE TABLE extra(id INTEGER PRIMARY KEY, value TEXT);
        INSERT INTO extra VALUES(1,'keep');
        CREATE TABLE child(parent INTEGER REFERENCES extra ON DELETE CASCADE);
        INSERT INTO child VALUES(1)",
    )
    .unwrap();
    edit_settings(&mut doc);
    save(&mut doc).unwrap();
    assert_eq!(
        db.query_row("SELECT value FROM extra", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "keep"
    );
    assert_eq!(
        db.query_row("SELECT parent FROM child", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}
