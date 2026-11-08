//! The wide graph (scope_ui::set_wide, owner 2026-11-07): the right panel
//! hidden, the 10 divisions stretched over the screen. Per scene: the
//! screen (wide-*.ppm), a fresh draw is the layout's own (replica), an
//! update after a trace change equals a fresh draw of the new rows, the
//! frame's right line at x 479, no panel pixel left; then the panel back.
use super::*;

pub fn run(ck: &mut Checker, out: &str) {
    let all = stock::scenes();
    for name in ["st01-default-td", "st03-auto-square", "st06-xcursors", "st07-ycursors"] {
        let Some(sc) = all.iter().find(|s| s.name == name) else {
            ck.fail("wide", format!("no scene {name}"));
            continue;
        };
        scope_ui::set_wide(true);
        stock::draw(sc);
        let scene = format!("wide/{name}");
        ck.check_replica(&scene);
        save(out, &format!("wide-{name}"));
        // The frame's right line and the former panel area.
        let fb = &emu().fb;
        let top = sc.m.top() as usize;
        let edge = (top + 1..top + sc.m.frame_h() as usize - 1).filter(|&y| fb[y * W + 479] == scope_ui::C_FRAME).count();
        let bar = (top + 2..300).filter(|&y| fb[y * W + 470] == scope_ui::C_BAR).count();
        if edge < (sc.m.frame_h() as usize).saturating_sub(30) || bar > 0 {
            ck.fail(&scene, format!("frame at x 479: {edge} rows; panel colour at x 470: {bar} rows"));
        }
        // An update after the trace moved = a fresh draw of the moved one.
        let g = scope_ui::graph();
        for r in g.tr.iter_mut() {
            if r.0 <= r.1 {
                *r = (r.0 + 9, r.1 + 9);
            }
        }
        let moved = g.tr;
        scope_ui::update(&sc.m);
        let inc = emu().fb.clone();
        host::reset(0);
        scope_ui::invalidate();
        stock::load(sc);
        scope_ui::graph().tr = moved;
        scope_ui::update(&sc.m);
        scope_ui::cells(&sc.m, &sc.cells);
        ck.same_as(&format!("{scene}/update"), &inc);
        println!("{scene}: frame rows at x 479 {edge}");
    }
    // The phosphor on the wide graph: its cells two px wide where the
    // column is.
    scope_ui::set_wide(false);
    let sc = &all[0];
    scope_ui::set_wide(true);
    stock::draw(sc);
    save(out, "wide-st01-panel-check");
    scope_ui::set_wide(false);
    stock::draw(sc);
    let fb = &emu().fb;
    let panel = (40..280).filter(|&y| fb[y * W + 470] != scope_ui::C_FILL).count();
    if panel == 0 {
        ck.fail("wide/back", "the panel did not come back".into());
    }
    // MF > Measure with 5 items: the selection row's 5 cells, then Clear
    // and Default (layout rules), on both graphs.
    if let Some(sc) = stock::emu_scenes().iter().find(|s| s.name == "e06-mf-measure") {
        let mut pv = stock::popvals(&sc.m);
        pv.meas = [0, 10, 9, 11, 17];
        pv.meas_n = 5;
        for w in [false, true] {
            scope_ui::set_wide(w);
            stock::draw_with(sc, &pv);
            let scene = format!("wide/measure-5{}", if w { "-wide" } else { "" });
            ck.check_replica(&scene);
            save(out, &scene.replace('/', "-"));
        }
        scope_ui::set_wide(false);
    } else {
        ck.fail("wide/measure-5", "no scene e06-mf-measure".into());
    }
}
