use super::engine::{Engine, HostCmd, InstId};
use super::parse::{parse_bundle, parse_patch, records, Canvas, Tok};
use super::*;
use std::collections::HashMap;
use std::sync::Arc;

fn build(text: &str) -> (Engine, InstId) {
    build_with(text, HashMap::new(), &[])
}

fn build_with(text: &str, abs: HashMap<&str, &str>, params: &[(&str, f32)]) -> (Engine, InstId) {
    let c = parse_patch("test", text).expect("parse");
    let parsed: HashMap<String, Arc<Canvas>> =
        abs.into_iter().map(|(k, v)| (k.to_string(), Arc::new(parse_patch(k, v).unwrap()))).collect();
    let mut e = Engine::new(7);
    let resolve = move |name: &str| parsed.get(name).cloned();
    let i = e.create_instance("test", &c, &resolve, params);
    (e, i)
}

fn prints(e: &mut Engine) -> Vec<String> {
    e.take_commands()
        .into_iter()
        .filter_map(|c| match c {
            HostCmd::Print { text, inst } if inst != u32::MAX => Some(text),
            _ => None,
        })
        .collect()
}

#[test]
fn tokens_follow_binbuf_rules() {
    let r = records("#X msg 10 20 1 \\, 0 3000;\r\n#X obj 1 2 s \\$0-gain;\n#X obj 1 2 f \\$2; #X text 0 0 a \\; b;");
    assert_eq!(r.len(), 4);
    assert_eq!(r[0][4..], [Tok::F(1.0), Tok::Comma, Tok::F(0.0), Tok::F(3000.0)]);
    assert_eq!(r[1][5], Tok::DollSym(sym("$0-gain")));
    assert_eq!(r[2][5], Tok::Dollar(2));
    assert!(r[3].contains(&Tok::Semi));
    // Numbers and symbols.
    let r = records("#X obj 0 0 + -; #X obj 0 0 function 1.46718e+007 -0.5 .5;");
    assert_eq!(r[0][4..], [Tok::S(sym("+")), Tok::S(sym("-"))]);
    assert_eq!(r[1][5..], [Tok::F(1.46718e7), Tok::F(-0.5), Tok::F(0.5)]);
    assert_eq!(fmt_g(1003.0), "1003");
    assert_eq!(fmt_g(0.25), "0.25");
    assert_eq!(fmt_g(1.0 / 3.0), "0.333333");
}

#[test]
fn messages_pass_depth_first_right_to_left() {
    let (mut e, _) = build(
        "#N canvas 0 0 400 400 12;\n#X obj 10 10 loadbang;\n#X msg 10 40 3 \\, 5;\n#X obj 10 70 t f f;\n#X obj 10 100 + 10;\n#X obj 10 130 print a;\n#X obj 80 100 print b;\n#X connect 0 0 1 0;\n#X connect 1 0 2 0;\n#X connect 2 0 3 0;\n#X connect 2 1 5 0;\n#X connect 3 0 4 0;\n",
    );
    assert_eq!(prints(&mut e), ["b: 3", "a: 13", "b: 5", "a: 15"]);
}

#[test]
fn subpatch_inlets_follow_x_order() {
    let (mut e, _) = build(
        "#N canvas 0 0 400 400 12;\n#X obj 10 10 loadbang;\n#X msg 10 40 1;\n#X msg 60 40 2;\n#N canvas 0 0 300 300 sub 0;\n#X obj 200 10 inlet;\n#X obj 20 10 inlet;\n#X obj 20 50 print left;\n#X obj 200 50 print right;\n#X connect 0 0 3 0;\n#X connect 1 0 2 0;\n#X restore 10 80 pd sub;\n#X connect 0 0 1 0;\n#X connect 0 0 2 0;\n#X connect 1 0 3 0;\n#X connect 2 0 3 1;\n",
    );
    assert_eq!(prints(&mut e), ["left: 1", "right: 2"]);
}

#[test]
fn abstractions_get_arguments_and_their_own_dollar_zero() {
    let adder = "#N canvas 0 0 300 300 12;\n#X obj 10 10 inlet;\n#X obj 10 40 + \\$1;\n#X obj 10 70 s \\$0-x;\n#X obj 10 100 r \\$0-x;\n#X obj 10 130 outlet;\n#X connect 0 0 1 0;\n#X connect 1 0 2 0;\n#X connect 3 0 4 0;\n";
    let (mut e, _) = build_with(
        "#N canvas 0 0 400 400 12;\n#X obj 10 10 loadbang;\n#X msg 10 40 5;\n#X obj 10 70 adder 100;\n#X obj 10 100 adder 1000;\n#X obj 10 130 print p;\n#X connect 0 0 1 0;\n#X connect 1 0 2 0;\n#X connect 2 0 3 0;\n#X connect 3 0 4 0;\n",
        HashMap::from([("adder", adder)]),
        &[],
    );
    assert_eq!(prints(&mut e), ["p: 1105"]);
}

#[test]
fn loadbang_order_is_abstractions_subpatches_then_own() {
    let abs = "#N canvas 0 0 300 300 12;\n#X obj 10 10 loadbang;\n#X obj 10 40 print abs;\n#X connect 0 0 1 0;\n";
    let (mut e, _) = build_with(
        "#N canvas 0 0 400 400 12;\n#X obj 10 10 loadbang;\n#X obj 10 40 print main;\n#N canvas 0 0 300 300 sub 0;\n#X obj 10 10 loadbang;\n#X obj 10 40 print sub;\n#X connect 0 0 1 0;\n#X restore 10 80 pd sub;\n#X obj 10 120 lb;\n#X connect 0 0 1 0;\n",
        HashMap::from([("lb", abs)]),
        &[],
    );
    assert_eq!(prints(&mut e), ["abs: bang", "sub: bang", "main: bang"]);
}

#[test]
fn clocks_fire_at_their_logical_time_one_frame_early() {
    let (mut e, _) = build(
        "#N canvas 0 0 400 400 12;\n#X obj 10 10 loadbang;\n#X obj 10 40 t b b;\n#X obj 10 70 del 100;\n#X obj 10 100 del 50;\n#X obj 200 40 timer;\n#X obj 200 100 print ms;\n#X connect 0 0 1 0;\n#X connect 1 0 2 0;\n#X connect 1 1 4 0;\n#X connect 2 0 3 0;\n#X connect 2 0 4 1;\n#X connect 3 0 4 1;\n#X connect 4 0 5 0;\n",
    );
    e.tick(0.0);
    e.tick(0.03);
    e.tick(0.06);
    assert!(prints(&mut e).is_empty());
    // 0.09 + step 0.03 > 0.1: the delay fires now, at logical time 0.1.
    e.tick(0.09);
    assert_eq!(prints(&mut e), ["ms: 100"]);
    e.tick(0.12);
    e.tick(0.15);
    assert_eq!(prints(&mut e), ["ms: 150"], "chained delay stays exact");
}

#[test]
fn metro_line_and_pause() {
    let (mut e, inst) = build(
        "#N canvas 0 0 400 400 12;\n#X obj 10 10 loadbang;\n#X msg 10 40 1;\n#X obj 10 70 metro 100;\n#X obj 10 100 print tick;\n#X msg 200 40 0 \\, 1 100;\n#X obj 200 70 line;\n#X obj 200 100 print line;\n#X connect 0 0 1 0;\n#X connect 0 0 4 0;\n#X connect 1 0 2 0;\n#X connect 2 0 3 0;\n#X connect 4 0 5 0;\n#X connect 5 0 6 0;\n",
    );
    e.run_for(0.0, 0.5, 0.01);
    let p = prints(&mut e);
    let ticks = p.iter().filter(|s| s.starts_with("tick")).count();
    assert!((5..=7).contains(&ticks), "{p:?}");
    let lines: Vec<f32> = p.iter().filter_map(|s| s.strip_prefix("line: ")).map(|s| s.parse().unwrap()).collect();
    assert_eq!(lines.first(), Some(&0.0));
    assert_eq!(lines.last(), Some(&1.0));
    assert!(lines.len() >= 6, "20 ms grain over 100 ms: {lines:?}");
    // Pausing the instance freezes its clocks.
    e.pause_instance(inst, true);
    e.run_for(0.51, 2.0, 0.01);
    assert!(prints(&mut e).is_empty());
    e.pause_instance(inst, false);
    e.run_for(2.01, 2.3, 0.01);
    assert!(prints(&mut e).iter().any(|s| s.starts_with("tick")));
}

#[test]
fn until_stops_through_a_feedback_loop() {
    let (mut e, _) = build(
        "#N canvas 0 0 400 400 12;\n#X obj 10 10 loadbang;\n#X msg 10 40 10;\n#X obj 10 70 until;\n#X obj 10 100 f;\n#X obj 60 100 + 1;\n#X obj 10 130 print i;\n#X obj 120 130 sel 3;\n#X connect 0 0 1 0;\n#X connect 1 0 2 0;\n#X connect 2 0 3 0;\n#X connect 3 0 4 0;\n#X connect 3 0 5 0;\n#X connect 3 0 6 0;\n#X connect 4 0 3 1;\n#X connect 6 0 2 1;\n",
    );
    assert_eq!(prints(&mut e), ["i: 0", "i: 1", "i: 2", "i: 3"]);
}

#[test]
fn coll_gate_route_pack() {
    let (mut e, _) = build(
        "#N canvas 0 0 400 400 12;\n#X obj 10 10 r go;\n#X obj 10 40 coll 11;\n#C list 0 music_cell_loopa;\n#C list 4 1 2;\n#X obj 10 70 t b s;\n#X msg 60 100 use \\$1;\n#X obj 10 130 print c;\n#X obj 200 10 r g;\n#X obj 200 40 gate 2;\n#X obj 200 70 print g1;\n#X obj 260 70 print g2;\n#X obj 300 10 r sel;\n#X obj 300 40 route 1 2;\n#X obj 300 70 print r1;\n#X obj 350 70 print rx;\n#X connect 0 0 1 0;\n#X connect 1 0 2 0;\n#X connect 1 0 4 0;\n#X connect 2 1 3 0;\n#X connect 3 0 4 0;\n#X connect 5 0 6 0;\n#X connect 6 0 7 0;\n#X connect 6 1 8 0;\n#X connect 9 0 10 0;\n#X connect 10 0 11 0;\n#X connect 10 2 12 0;\n",
    );
    e.send("go", &Msg::float(0.0));
    let p = prints(&mut e);
    assert_eq!(p, ["c: use music_cell_loopa", "c: symbol music_cell_loopa"]);
    e.send("go", &Msg::float(4.0));
    assert!(prints(&mut e).contains(&"c: 1 2".to_string()));
    // gate: shut by default; 2 opens the second outlet. (Control on inlet 1
    // is tested through the engine's receive bus by a list.)
    e.send("g", &Msg::float(9.0));
    assert!(prints(&mut e).is_empty());
    e.send("sel", &Msg::float(1.0));
    e.send("sel", &Msg::float(7.0));
    assert_eq!(prints(&mut e), ["r1: bang", "rx: 7"]);
}

#[test]
fn play_steals_crossfades_and_picks_group_members() {
    let (mut e, _) = build(
        "#N canvas 0 0 400 400 12;\n#X obj 10 10 loadbang;\n#X msg 10 40 fadeout 2 \\, fadein 1;\n#X obj 10 100 dplay mysounds 1;\n#X obj 100 10 r go;\n#X obj 200 10 group g x y z;\n#X obj 200 40 r grp;\n#X obj 200 70 play g 1 inorder;\n#X connect 0 0 1 0;\n#X connect 1 0 2 0;\n#X connect 3 0 2 0;\n#X connect 5 0 6 0;\n",
    );
    e.take_commands();
    e.send("go", &Msg::any("use", vec![Atom::S(sym("loopa"))]));
    e.send("go", &Msg::any("start", vec![]));
    let c = e.take_commands();
    let HostCmd::Start { voice: v1, sound, params, .. } = &c[0] else { panic!("{c:?}") };
    assert_eq!(&**sound, "loopa");
    assert_eq!(params.len(), 2);
    let v1 = *v1;
    e.send("go", &Msg::any("use", vec![Atom::S(sym("loopb"))]));
    e.send("go", &Msg::any("start", vec![]));
    let c = e.take_commands();
    assert_eq!(c[0], HostCmd::Stop { voice: v1 });
    let HostCmd::Start { voice: v2, sound, .. } = &c[1] else { panic!("{c:?}") };
    assert_eq!(&**sound, "loopb");
    let v2 = *v2;
    e.send("go", &Msg::any("gain", vec![Atom::F(0.5)]));
    assert!(matches!(&e.take_commands()[0], HostCmd::Param { voice, value: Atom::F(v), .. } if *voice == v2 && *v == 0.5));
    // A finished voice frees the slot: no steal.
    e.voice_ended(v2);
    e.send("go", &Msg::any("start", vec![]));
    let c = e.take_commands();
    assert!(matches!(&c[..], [HostCmd::Start { .. }]), "{c:?}");
    // inorder group picks.
    let mut picked = vec![];
    for _ in 0..4 {
        e.send("grp", &Msg::any("start", vec![]));
        for c in e.take_commands() {
            if let HostCmd::Start { sound, .. } = c {
                picked.push(sound.to_string());
            }
        }
    }
    assert_eq!(picked, ["x", "y", "z", "x"]);
}

#[test]
fn params_reach_gfparam_and_fparam() {
    let (mut e, inst) = build_with(
        "#N canvas 0 0 400 400 12;\n#X obj 10 10 gfparam cell_level;\n#X obj 10 40 change;\n#X obj 10 70 print g;\n#X obj 200 10 fparam gain;\n#X obj 200 40 print f;\n#X connect 0 0 1 0;\n#X connect 1 0 2 0;\n#X connect 3 0 4 0;\n",
        HashMap::new(),
        &[("gain", 0.7)],
    );
    assert_eq!(prints(&mut e), ["f: 0.7"]);
    e.set_global_param("cell_level", 1.2);
    e.set_global_param("cell_level", 1.2);
    e.set_global_param_hash(fnv("cell_level"), 1.3);
    assert_eq!(prints(&mut e), ["g: 1.2", "g: 1.3"]);
    e.set_instance_param(inst, "gain", 0.25);
    assert_eq!(prints(&mut e), ["f: 0.25"]);
    e.destroy_instance(inst);
    e.set_global_param("cell_level", 5.0);
    assert!(prints(&mut e).is_empty());
    assert_eq!(e.object_count(), 0);
}

#[test]
fn function_matches_the_cell_curves() {
    let args: Vec<Atom> = "200 100 3549 0 4080 0 1 0 1.1 0 0.955555 0.0842105 0.844444 0.131579 0.833333 0.194737 0.822222 0.310526 0.877778 0.468421 0.944445 0.7 1 1 1"
        .split_whitespace()
        .map(|s| Atom::F(s.parse().unwrap()))
        .collect();
    let f = objects::Function::new(&args);
    for x in [0.0, 0.05, 0.2, 0.5, 0.9, 1.0, 1.5] {
        assert!((f.eval(x) - crate::cell_audio::MOTION_PITCH.eval(x)).abs() < 1e-5, "x {x}");
    }
    // Domain and output range (`1 500 43600 0`).
    let args: Vec<Atom> = "200 100 3549 0 4080 0 200 0 1 0 1 1 0".split_whitespace().map(|s| Atom::F(s.parse().unwrap())).collect();
    let g = objects::Function::new(&args);
    assert!((g.eval(100.0) - 0.5).abs() < 1e-6);
}

#[test]
fn binops_follow_pd() {
    use objects::binop_eval as b;
    assert_eq!(b("/", 1.0, 0.0), 0.0);
    assert_eq!(b("%", -7.0, 3.0), -1.0);
    assert_eq!(b("mod", -7.0, 3.0), 2.0);
    assert_eq!(b("div", -7.0, 3.0), -3.0);
    assert_eq!(b("div", 70.0, 32.0), 2.0);
    assert_eq!(b("pow", -2.0, 2.0), 0.0);
    assert_eq!(b("&&", 1.0, 0.5), 0.0);
}

#[test]
fn bundles_parse() {
    let mut b = b"EAPD".to_vec();
    for v in [3u32, 2, 0, 0, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for (name, data) in [("top", "#N canvas 0 0 1 1 12;\n#X obj 1 1 mult;\n"), ("mult", "#N canvas 0 0 1 1 12;\n")] {
        let mut n = name.as_bytes().to_vec();
        n.push(0);
        b.extend_from_slice(&(n.len() as u32).to_le_bytes());
        b.extend_from_slice(&n);
        b.extend_from_slice(b"pd\0\0");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(data.as_bytes());
    }
    let e = parse_bundle(&b).unwrap();
    assert_eq!(e.len(), 2);
    assert_eq!(e[0].name, "top");
    assert_eq!(e[1].tag, "pd");
    let top = parse_patch("top", &String::from_utf8_lossy(&e[0].data)).unwrap();
    assert_eq!(top.boxes.len(), 1);
}

// ---------------------------------------------------------------------------
// Installed data
// ---------------------------------------------------------------------------

fn installed() -> Option<(Arc<library::PdLibrary>, Arc<crate::game_audio::AudioLibrary>)> {
    let install = crate::spore_install::discover().ok()?;
    let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
    Some((Arc::new(library::PdLibrary::from_index(&index)), Arc::new(crate::game_audio::AudioLibrary::from_index(&index))))
}

#[test]
#[ignore = "needs the installed Spore audio packages"]
fn installed_cell_music_matches_retail_behaviour() {
    let Some((lib, audio)) = installed() else { return };
    let mut h = trace::Headless::new(lib, audio, 1_489_853_723);
    h.start("cellgame_seed_music").unwrap();
    trace::run_cell_session(&mut h, trace::CELL_MATE_AT + 12.0);
    let checks = trace::check_cell(&h.events);
    let failed: Vec<_> = checks.iter().filter(|c| !c.ok).map(|c| c.what.clone()).collect();
    for c in &checks {
        eprintln!("[{}] {}", if c.ok { "PASS" } else { "FAIL" }, c.what);
    }
    let real_errors: Vec<_> = h.engine.errors.iter().filter(|e| !e.contains("not implemented")).collect();
    assert!(real_errors.is_empty(), "{real_errors:?}");
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
#[ignore = "needs the installed Spore audio packages"]
fn installed_stage_patches_run() {
    let Some((lib, audio)) = installed() else { return };
    // Modes name their patches.
    let creature = lib.mode("mixmode_creaturegame").expect("mixmode_creaturegame");
    assert!(creature.startsounds.contains(&fnv("music_creature_modified")), "{creature:?}");
    let tribe = lib.mode("mixmode_tribegame").expect("mixmode_tribegame");
    assert!(tribe.startsounds.contains(&fnv("tribe_drums")) && tribe.startsounds.contains(&fnv("music_tribe_social")));
    for mode in ["mixmode_global", "mixmode_cellgame", "mixmode_creaturegame", "mixmode_tribegame", "mixmode_civgame", "mixmode_spacegame"] {
        let def = lib.mode(mode).unwrap_or_default();
        let kinds: Vec<String> = def
            .startsounds
            .iter()
            .map(|id| match (lib.patch_sound(*id), audio.resolve(&format!("#{id:08X}"))) {
                (Some(p), _) => format!("{id:08X}=patch:{:08X}", p.bundle),
                (None, Ok(d)) => format!("{id:08X}=sound(looped {})", d.looped),
                (None, Err(_)) => format!("{id:08X}=missing"),
            })
            .collect();
        eprintln!("{mode}: {kinds:?} pause {:?}", def.pausegroups);
    }
    let pause = lib.mode("mixmode_userpause").unwrap();
    assert!(pause.pausegroups.contains(&fnv("userpause_group")));
    assert_eq!(lib.patch_sound(fnv("cellgame_seed_music")).map(|p| p.bundle), Some(fnv("cellgame_seed_music6")));
    assert!(lib.instrument("harp").is_some_and(|i| i.zones.len() >= 10));
    for name in ["cell_motion", "cell_amb", "music_creature_modified", "tribe_drums", "music_tribe_social", "mix_transitions", "mix_lowcut", "mix_event_cell_vo"] {
        let mut h = trace::Headless::new(lib.clone(), audio.clone(), 99);
        h.start(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        for k in 0..(40 * 60) {
            let t = k as f32 / 60.0;
            h.engine.set_global_param("cell_speed", (t * 0.2).sin().abs());
            h.engine.set_global_param("cell_level", 1.0 + (t / 40.0));
            h.engine.set_global_param("creature_health", 1.0);
            h.engine.set_global_param("timeofday", 9.0);
            h.engine.set_global_param("npctribecount", 2.0);
            // A fight from 10 to 25 s (the idle creature score is silent:
            // its music comes from events, fights, social, dawn/dusk).
            let fight = if (10.0..25.0).contains(&t) { 1.0 } else { 0.0 };
            h.engine.set_global_param("infight", fight);
            h.engine.set_global_param("ui_fight", fight);
            h.step(1.0 / 60.0);
        }
        let starts: Vec<String> = h
            .events
            .iter()
            .filter_map(|e| match &e.ev {
                trace::Ev::Start { sound, .. } => Some(sound.clone()),
                _ => None,
            })
            .collect();
        let errors: Vec<_> = h.engine.errors.iter().filter(|e| !e.contains("not implemented") && !e.contains("couldn't create")).collect();
        eprintln!("{name}: {} objects, {} starts ({:?}...), {} engine messages {:?}", h.engine.object_count(), starts.len(), starts.iter().take(6).collect::<Vec<_>>(), h.engine.errors.len(), h.engine.errors.iter().take(8).collect::<Vec<_>>());
        assert!(errors.len() < 5, "{name}: {errors:?}");
        if matches!(name, "cell_motion" | "cell_amb" | "music_creature_modified" | "tribe_drums") {
            assert!(!starts.is_empty(), "{name} played nothing");
        }
    }
}
