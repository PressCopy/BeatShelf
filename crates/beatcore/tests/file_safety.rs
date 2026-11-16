//! End-to-end tests for the rules in ARCHITECTURE.md, "File safety".
//! Each test builds a throw-away library in a temp folder.

use beatcore::db::Db;
use beatcore::ops::{execute, plan_undo, BatchStatus, DoneOp, ExecError, ExecOptions, IssueCode, Journal, MemoryJournal, PlanBuilder};
use beatcore::template::default_template;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

struct Lib {
    _dir: tempfile::TempDir,
    root: PathBuf,
    inbox: PathBuf,
    outside: PathBuf,
}

fn lib() -> Lib {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Beats Library");
    let inbox = root.join("00_Inbox");
    let outside = dir.path().join("Elsewhere");
    fs::create_dir_all(&inbox).unwrap();
    fs::create_dir_all(&outside).unwrap();
    Lib { _dir: dir, root, inbox, outside }
}

fn write(p: &Path, content: &[u8]) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    let mut f = fs::File::create(p).unwrap();
    f.write_all(content).unwrap();
}

fn codes(issues: &[beatcore::ops::Issue]) -> Vec<IssueCode> {
    issues.iter().map(|i| i.code).collect()
}

/// Ableton project folder with one internal and one external sample.
fn ableton_project(parent: &Path, name: &str, external: &Path) -> PathBuf {
    use flate2::write::GzEncoder;
    let proj = parent.join(format!("{name} Project"));
    fs::create_dir_all(proj.join("Ableton Project Info")).unwrap();
    write(&proj.join("Samples/Imported/kick.wav"), b"RIFFkick");
    let ext = external.to_string_lossy().replace('\\', "/");
    let xml = format!(
        "<Ableton><SampleRef><FileRef><RelativePathType Value=\"3\"/><RelativePath Value=\"Samples/Imported/kick.wav\"/><Path Value=\"/old/place/kick.wav\"/></FileRef></SampleRef>\
         <SampleRef><FileRef><RelativePathType Value=\"1\"/><RelativePath Value=\"\"/><Path Value=\"{ext}\"/></FileRef></SampleRef></Ableton>"
    );
    let f = fs::File::create(proj.join(format!("{name}.als"))).unwrap();
    let mut gz = GzEncoder::new(f, flate2::Compression::fast());
    gz.write_all(xml.as_bytes()).unwrap();
    gz.finish().unwrap();
    proj
}

#[test]
fn library_creation_and_undo_leaves_no_trace() {
    let l = lib();
    let tpl = default_template();
    let mut b = PlanBuilder::new("Create library", vec![l.root.clone()]);
    for d in tpl.dirs_to_create() {
        b.create_dir(l.root.join(d));
    }
    let plan = b.build();
    assert!(plan.is_executable(), "{:?}", plan.blockers);
    // 00_Inbox already exists, so it is not part of the plan.
    assert!(!plan.steps.iter().any(|s| matches!(s, beatcore::ops::Step::CreateDir { path } if path.ends_with("00_Inbox"))));

    let mut j = MemoryJournal::default();
    let rep = execute(&plan, &mut j, &ExecOptions::default()).unwrap();
    assert!(l.root.join("02_Samples_Kits/Ableton Racks").is_dir());
    assert!(l.root.join("99_Archive").is_dir());

    // A file the user put there after creation must survive the undo.
    write(&l.root.join("04_Collabs/notes.txt"), b"keep me");
    let ops = j.ops_of(rep.batch_id.unwrap());
    let undo = plan_undo(rep.batch_id.unwrap(), "Undo", &ops, vec![l.root.clone()]);
    assert!(codes(&undo.warnings).contains(&IssueCode::DirNotEmpty));
    let rep2 = execute(&undo, &mut j, &ExecOptions::default()).unwrap();
    assert!(!rep2.skipped.is_empty());
    assert!(!l.root.join("02_Samples_Kits").exists());
    assert!(l.root.join("04_Collabs/notes.txt").exists());
    assert!(l.root.join("00_Inbox").is_dir(), "pre-existing folder untouched");
}

#[test]
fn sort_into_beat_then_undo() {
    let l = lib();
    write(&l.inbox.join("Night_140bpm_Am.wav"), b"RIFFaudio");
    write(&l.inbox.join("Night.flp"), b"FLhd");
    let beat = l.root.join("01_Beats/2026/Night_140bpm_Am");

    let mut b = PlanBuilder::new("Sort Night", vec![l.root.clone()]);
    b.create_dir(beat.join("Bounces")).create_dir(beat.join("Project_FL"));
    b.move_item(l.inbox.join("Night_140bpm_Am.wav"), beat.join("Bounces/Night_140bpm_Am.wav"));
    b.move_item(l.inbox.join("Night.flp"), beat.join("Project_FL/Night_140bpm_Am.flp"));
    let plan = b.build();
    assert!(plan.is_executable());
    assert!(codes(&plan.warnings).contains(&IssueCode::FlSamplePaths));

    let mut db = Db::open_in_memory().unwrap();
    let rep = execute(&plan, &mut db, &ExecOptions::default()).unwrap();
    assert_eq!(fs::read(beat.join("Bounces/Night_140bpm_Am.wav")).unwrap(), b"RIFFaudio");
    assert!(!l.inbox.join("Night.flp").exists());
    assert_eq!(rep.movements().len(), 2);

    let id = rep.batch_id.unwrap();
    let ops = db.batch_ops(id).unwrap();
    assert_eq!(ops.len(), 2 + 5, "five created folders + two moves: {ops:?}");
    let undo = plan_undo(id, "Undo", &ops, vec![l.root.clone()]);
    assert!(undo.is_executable(), "{:?}", undo.blockers);
    execute(&undo, &mut db, &ExecOptions::default()).unwrap();
    assert_eq!(fs::read(l.inbox.join("Night_140bpm_Am.wav")).unwrap(), b"RIFFaudio");
    assert!(l.inbox.join("Night.flp").exists());
    assert!(!l.root.join("01_Beats").exists(), "folders created by the batch are gone");
    let batches = db.list_batches(10).unwrap();
    let original = batches.iter().find(|b| b.id == id).unwrap();
    assert!(original.undone_by.is_some());
    assert!(!original.can_undo);
}

#[test]
fn never_overwrites_including_case_and_unicode_variants() {
    let l = lib();
    let dest_dir = l.root.join("01_Beats/X");
    write(&dest_dir.join("Мой бит.wav"), b"original");
    write(&l.inbox.join("new.wav"), b"new");
    // Destination differs only by case and Unicode normalisation (NFD).
    let mut b = PlanBuilder::new("Clash", vec![l.root.clone()]);
    b.move_item(l.inbox.join("new.wav"), dest_dir.join("мои\u{0306} БИТ.wav"));
    let plan = b.build();
    assert_eq!(codes(&plan.blockers), vec![IssueCode::DestinationExists]);
    assert_eq!(fs::read(dest_dir.join("Мой бит.wav")).unwrap(), b"original");

    // Two items into the same target in one plan
    write(&l.inbox.join("a.wav"), b"a");
    write(&l.inbox.join("b.wav"), b"b");
    let mut b = PlanBuilder::new("Dup", vec![l.root.clone()]);
    b.move_item(l.inbox.join("a.wav"), dest_dir.join("same.wav"));
    b.move_item(l.inbox.join("b.wav"), dest_dir.join("SAME.wav"));
    assert_eq!(codes(&b.build().blockers), vec![IssueCode::DestinationDuplicate]);
}

#[test]
fn stale_plan_is_refused_without_touching_anything() {
    let l = lib();
    write(&l.inbox.join("a.wav"), b"a");
    let target = l.root.join("a.wav");
    let mut b = PlanBuilder::new("Move", vec![l.root.clone()]);
    b.move_item(l.inbox.join("a.wav"), &target);
    let plan = b.build();
    assert!(plan.is_executable());
    // Someone drops a file at the destination while the plan dialog is open.
    write(&target, b"someone else's");
    let mut j = MemoryJournal::default();
    match execute(&plan, &mut j, &ExecOptions::default()) {
        Err(ExecError::Stale(issues)) => assert_eq!(codes(&issues), vec![IssueCode::DestinationExists]),
        other => panic!("expected stale, got {other:?}"),
    }
    assert_eq!(fs::read(&target).unwrap(), b"someone else's");
    assert!(l.inbox.join("a.wav").exists());
    assert!(j.batches.is_empty(), "no batch is opened for a stale plan");
}

#[test]
fn failure_mid_batch_rolls_back_everything() {
    let l = lib();
    for n in ["1.wav", "2.wav", "3.wav"] {
        write(&l.inbox.join(n), n.as_bytes());
    }
    let dest = l.root.join("01_Beats/Y/Bounces");
    let mut b = PlanBuilder::new("Three", vec![l.root.clone()]);
    b.create_dir(&dest);
    for n in ["1.wav", "2.wav", "3.wav"] {
        b.move_item(l.inbox.join(n), dest.join(n));
    }
    let plan = b.build();
    let mut j = MemoryJournal::default();
    let err = execute(&plan, &mut j, &ExecOptions { fail_at_step: Some(3), ..Default::default() }).unwrap_err();
    match err {
        ExecError::Failed { step, rolled_back, .. } => {
            assert_eq!(step, 3);
            assert!(rolled_back);
        }
        other => panic!("{other:?}"),
    }
    for n in ["1.wav", "2.wav", "3.wav"] {
        assert_eq!(fs::read(l.inbox.join(n)).unwrap(), n.as_bytes());
    }
    assert!(!l.root.join("01_Beats").exists());
    assert_eq!(j.batches[0].2, BatchStatus::RolledBack);
}

#[test]
fn ableton_set_never_leaves_its_project() {
    let l = lib();
    let external = l.outside.join("Splice/snare.wav");
    write(&external, b"snare");
    let proj = ableton_project(&l.inbox, "Night", &external);

    // Moving only the .als out of the project is blocked.
    let mut b = PlanBuilder::new("Tear", vec![l.root.clone()]);
    b.move_item(proj.join("Night.als"), l.root.join("Night.als"));
    assert_eq!(codes(&b.build().blockers), vec![IssueCode::AbletonTear]);

    // So is moving its Samples folder away.
    let mut b = PlanBuilder::new("Tear samples", vec![l.root.clone()]);
    b.move_item(proj.join("Samples"), l.root.join("Samples"));
    assert_eq!(codes(&b.build().blockers), vec![IssueCode::AbletonTear]);

    // Renaming inside the project is fine.
    let mut b = PlanBuilder::new("Rename inside", vec![l.root.clone()]);
    b.move_item(proj.join("Night.als"), proj.join("Night v2.als"));
    assert!(b.build().is_executable());

    // The whole folder moves, with a warning about the external sample.
    let target = l.root.join("01_Beats/Night/Project_Ableton/Night Project");
    let mut b = PlanBuilder::new("Move project", vec![l.root.clone()]);
    b.create_dir(target.parent().unwrap());
    b.move_item(&proj, &target);
    let plan = b.build();
    assert!(plan.is_executable(), "{:?}", plan.blockers);
    let w = plan.warnings.iter().find(|w| w.code == IssueCode::AbletonExternalSamples).expect("warning");
    assert_eq!(w.items.len(), 1);
    assert!(w.items[0].ends_with("Splice/snare.wav"));
    let mut j = MemoryJournal::default();
    execute(&plan, &mut j, &ExecOptions::default()).unwrap();
    assert!(target.join("Samples/Imported/kick.wav").exists());
    assert!(target.join("Night.als").exists());
}

#[test]
fn cross_volume_move_copies_verifies_and_trashes() {
    let l = lib();
    let src_dir = l.inbox.join("Stems Night");
    write(&src_dir.join("kick.wav"), &vec![7u8; 300_000]);
    write(&src_dir.join("sub/808.wav"), b"808");
    write(&l.inbox.join("bounce.wav"), b"bounce");
    let dest = l.root.join("01_Beats/Z");
    let mut b = PlanBuilder::new("Copy", vec![l.root.clone()]);
    b.create_dir(&dest);
    b.move_item(&src_dir, dest.join("Stems"));
    b.move_item(l.inbox.join("bounce.wav"), dest.join("bounce.wav"));
    let plan = b.build();
    let mut j = MemoryJournal::default();
    let opts = ExecOptions { force_copy: true, ..Default::default() };
    let rep = execute(&plan, &mut j, &opts).unwrap();
    assert!(rep.ops.iter().any(|o| matches!(o, DoneOp::CopiedAndTrashed { .. })));
    assert_eq!(fs::read(dest.join("Stems/kick.wav")).unwrap().len(), 300_000);
    assert_eq!(fs::read(dest.join("Stems/sub/808.wav")).unwrap(), b"808");
    assert!(!src_dir.exists(), "original went to the trash");
    // No temp leftovers next to the destination.
    let leftovers: Vec<_> = fs::read_dir(&dest)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".beatshelf-tmp"))
        .collect();
    assert!(leftovers.is_empty());

    // Undo brings it back the same way.
    let undo = plan_undo(rep.batch_id.unwrap(), "Undo", &j.ops_of(rep.batch_id.unwrap()), vec![l.root.clone()]);
    execute(&undo, &mut j, &opts).unwrap();
    assert_eq!(fs::read(src_dir.join("sub/808.wav")).unwrap(), b"808");
    assert_eq!(fs::read(l.inbox.join("bounce.wav")).unwrap(), b"bounce");
}

#[test]
fn guards_against_bad_destinations() {
    let l = lib();
    write(&l.inbox.join("Folder/a.wav"), b"a");
    // Into itself
    let mut b = PlanBuilder::new("Self", vec![l.root.clone()]);
    b.move_item(l.inbox.join("Folder"), l.inbox.join("Folder/Inner"));
    assert!(codes(&b.build().blockers).contains(&IssueCode::IntoItself));
    // Outside the library
    let mut b = PlanBuilder::new("Out", vec![l.root.clone()]);
    b.move_item(l.inbox.join("Folder/a.wav"), l.outside.join("a.wav"));
    assert_eq!(codes(&b.build().blockers), vec![IssueCode::OutsideLibrary]);
    // Missing parent folder (planner forgot create_dir)
    let mut b = PlanBuilder::new("Parent", vec![l.root.clone()]);
    b.move_item(l.inbox.join("Folder/a.wav"), l.root.join("nope/a.wav"));
    assert_eq!(codes(&b.build().blockers), vec![IssueCode::ParentMissing]);
    // Source missing
    let mut b = PlanBuilder::new("Gone", vec![l.root.clone()]);
    b.move_item(l.inbox.join("ghost.wav"), l.root.join("ghost.wav"));
    assert_eq!(codes(&b.build().blockers), vec![IssueCode::SourceMissing]);
    // Very long destination path
    let deep = l.root.join("x".repeat(120)).join("y".repeat(120));
    let mut b = PlanBuilder::new("Long", vec![l.root.clone()]);
    b.create_dir(&deep);
    b.move_item(l.inbox.join("Folder/a.wav"), deep.join("a.wav"));
    assert!(codes(&b.build().warnings).contains(&IssueCode::LongPath));
}

#[test]
fn case_only_rename() {
    let l = lib();
    write(&l.inbox.join("night.wav"), b"n");
    let mut b = PlanBuilder::new("Case", vec![l.root.clone()]);
    b.move_item(l.inbox.join("night.wav"), l.inbox.join("Night.wav"));
    let plan = b.build();
    assert!(plan.is_executable(), "{:?}", plan.blockers);
    let mut j = MemoryJournal::default();
    execute(&plan, &mut j, &ExecOptions::default()).unwrap();
    let names: Vec<String> = fs::read_dir(&l.inbox).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into()).collect();
    assert_eq!(names, vec!["Night.wav".to_string()]);
}

#[test]
fn db_journal_survives_reopen_and_marks_crashed_batches() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lib.sqlite");
    {
        let mut db = Db::open(&path).unwrap();
        let id = db.begin_batch("Interrupted", None).unwrap();
        db.record_op(id, 0, &DoneOp::CreatedDir { path: dir.path().join("x") }).unwrap();
        // no finish_batch: simulates a crash
    }
    let db = Db::open(&path).unwrap();
    let b = &db.list_batches(10).unwrap()[0];
    assert_eq!(b.status, "failed");
    assert!(b.can_undo, "recorded ops of a crashed batch stay undoable");
}

// ── Copy into the library (drag and drop, "Add files") ──

#[test]
fn copy_from_anywhere_keeps_the_original_and_undo_trashes_only_the_copy() {
    let l = lib();
    let contract = l.outside.join("Contract Night.pdf");
    write(&contract, b"%PDF contract");
    let trackout = l.outside.join("Night trackout");
    write(&trackout.join("kick.wav"), b"RIFFkick");
    write(&trackout.join("bass.wav"), b"RIFFbass");
    let beat = l.root.join("01_Beats/2026/Night");
    fs::create_dir_all(&beat).unwrap();

    let mut b = PlanBuilder::new("Add files", vec![l.root.clone()]);
    b.create_dir(beat.join("Contracts"));
    b.create_dir(beat.join("Stems_Trackouts"));
    b.copy_item(&contract, beat.join("Contracts/Contract Night.pdf"));
    b.copy_item(&trackout, beat.join("Stems_Trackouts/Night trackout"));
    let plan = b.build();
    assert!(plan.is_executable(), "{:?}", plan.blockers);

    let mut j = MemoryJournal::default();
    let report = execute(&plan, &mut j, &ExecOptions::default()).unwrap();
    assert_eq!(fs::read(beat.join("Contracts/Contract Night.pdf")).unwrap(), b"%PDF contract");
    assert_eq!(fs::read(beat.join("Stems_Trackouts/Night trackout/bass.wav")).unwrap(), b"RIFFbass");
    // Originals untouched.
    assert!(contract.exists() && trackout.join("kick.wav").exists());
    assert!(report.movements().is_empty(), "a copy is not a move");

    let batch = report.batch_id.unwrap();
    let undo = plan_undo(batch, "Undo", &j.ops_of(batch), vec![l.root.clone()]);
    assert!(undo.is_executable(), "{:?}", undo.blockers);
    execute(&undo, &mut j, &ExecOptions::default()).unwrap();
    assert!(!beat.join("Contracts").exists());
    assert!(!beat.join("Stems_Trackouts").exists());
    assert!(contract.exists() && trackout.join("bass.wav").exists());
}

#[test]
fn copy_never_overwrites_and_never_writes_outside_the_library() {
    let l = lib();
    let src = l.outside.join("a.pdf");
    write(&src, b"new");
    let existing = l.root.join("01_Beats/Night/Contracts/a.pdf");
    write(&existing, b"old");

    let mut b = PlanBuilder::new("Add", vec![l.root.clone()]);
    b.copy_item(&src, &existing);
    assert!(codes(&b.build().blockers).contains(&IssueCode::DestinationExists));

    let mut b = PlanBuilder::new("Add", vec![l.root.clone()]);
    b.copy_item(&src, l.outside.join("b.pdf"));
    assert!(codes(&b.build().blockers).contains(&IssueCode::OutsideLibrary));
    assert_eq!(fs::read(&existing).unwrap(), b"old");
}

#[test]
fn moving_from_outside_is_still_refused() {
    let l = lib();
    let src = l.outside.join("a.wav");
    write(&src, b"RIFF");
    let mut b = PlanBuilder::new("Move", vec![l.root.clone()]);
    b.move_item(&src, l.inbox.join("a.wav"));
    assert!(codes(&b.build().blockers).contains(&IssueCode::OutsideLibrary));
}

#[test]
fn failed_copy_batch_rolls_back_finished_copies() {
    let l = lib();
    let a = l.outside.join("a.pdf");
    let c = l.outside.join("c.pdf");
    write(&a, b"a");
    write(&c, b"c");
    let dest = l.root.join("01_Beats/Night/Contracts");
    let mut b = PlanBuilder::new("Add", vec![l.root.clone()]);
    b.create_dir(&dest);
    b.copy_item(&a, dest.join("a.pdf"));
    b.copy_item(&c, dest.join("c.pdf"));
    let plan = b.build();
    let mut j = MemoryJournal::default();
    let err = execute(&plan, &mut j, &ExecOptions { fail_at_step: Some(2), ..Default::default() }).unwrap_err();
    assert!(matches!(err, ExecError::Failed { rolled_back: true, .. }));
    assert!(!dest.join("a.pdf").exists());
    assert!(a.exists() && c.exists());
}

#[test]
fn library_rules_block_a_plan_even_if_the_disk_allows_it() {
    let l = lib();
    let src = l.inbox.join("tag.flp");
    write(&src, b"FLhd");
    let dest = l.root.join("07_Voicetags/Tag");
    let mut b = PlanBuilder::new("Sort", vec![l.root.clone()]);
    b.create_dir(&dest);
    b.block(beatcore::ops::Issue::new(IssueCode::VoicetagFileType, &src));
    let plan = b.build();
    assert!(!plan.is_executable());
    let mut j = MemoryJournal::default();
    assert!(matches!(execute(&plan, &mut j, &ExecOptions::default()), Err(ExecError::Stale(_))));
    assert!(!dest.exists() && src.exists());
}

// ── Deleting an item: its folder goes to the trash through a plan ──

#[test]
fn deleting_an_item_trashes_its_folder_and_warns_about_files() {
    let l = lib();
    let beat = l.root.join("01_Beats/2026/Night");
    write(&beat.join("Bounces/Night.wav"), b"RIFF");
    write(&beat.join("Project_FL/Night.flp"), b"FLhd");
    let other = l.root.join("01_Beats/2026/Velvet/Bounces/Velvet.wav");
    write(&other, b"RIFF");

    let mut b = PlanBuilder::new("Delete", vec![l.root.clone()]);
    b.warn(beatcore::ops::Issue::new(IssueCode::DeletesFiles, &beat).detail("2"));
    b.trash_item(&beat);
    let plan = b.build();
    assert!(plan.is_executable(), "{:?}", plan.blockers);
    assert!(codes(&plan.warnings).contains(&IssueCode::DeletesFiles));

    let mut j = MemoryJournal::default();
    execute(&plan, &mut j, &ExecOptions::default()).unwrap();
    assert!(!beat.exists());
    assert!(other.exists(), "neighbours are untouched");

    // Nothing outside the library can be trashed.
    let outside = l.outside.join("keep");
    write(&outside.join("a.wav"), b"RIFF");
    let mut b = PlanBuilder::new("Delete", vec![l.root.clone()]);
    b.trash_item(&outside);
    assert!(codes(&b.build().blockers).contains(&IssueCode::OutsideLibrary));
    assert!(outside.join("a.wav").exists());
}
