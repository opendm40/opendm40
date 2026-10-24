//! The screens of the user manual (docs/manual/img/*.png), 1x, with
//! ordinary values and the defaults of a new meter, not the worst cases of
//! the matrix. Synthetic data, drawn by the firmware's own code; each screen
//! also goes through the layout rules.
//!   tools/ui-render/run.sh OUT --manual   (then optimise into docs/manual/img/)
use super::*;

fn save1(out: &str, name: &str) {
    host::save_ppm(&format!("{out}/{name}.ppm"), 1);
}

pub fn run(out: &str) -> usize {
    std::fs::create_dir_all(out).unwrap();
    let mut ck = Checker { cases: 0, violations: 0, seen: Default::default() };
    let page = vec![("screen", Bx::new(0, 0, 480, 320)), ("bar", Bx::new(0, 0, 480, 40))];

    // Settings pages with the defaults of a new meter.
    host::reset(0);
    ui::settings(0, &["5 min", "30 min", "55%", "50%", "ON", "Multimeter", "Auto (per view)", "Stock", ""]);
    ck.check_opts("manual/settings1", &page, false);
    save1(out, "settings1");
    host::reset(0);
    ui::settings(1, &["MED  9.4/s", "OFF", "Bargraph", "C (Celsius)", "Off (stock)", "ON", "Corners", "", ""]);
    ck.check_opts("manual/settings2", &page, false);
    save1(out, "settings2");
    host::reset(0);
    ui::settings(2, &["ON", "ON", "Loud", ui::usb_disk_name(true, true), "OFF (stock)", ""]);
    ck.check_opts("manual/settings3", &page, false);
    save1(out, "settings3");

    // Voltage range page: AUTO+ selected, DC, the AUTO+ note, Wave.
    host::reset(0);
    ui::range_page(Button::Volt, Some(5), 0, Some(("AUTO+ recommended: never steps to mV", false)));
    ui::wave_button();
    ck.check_opts("manual/range-v", &page, false);
    save1(out, "range-v");

    // Hold & Limits of DC V: limits 4.8 .. 5.2 V on.
    host::reset(0);
    ui::list_page("Hold & Limits: DC V", &ui::LIMIT_ROWS, &["OFF", "ON", "5.2000 V", "4.8000 V", "", "", "", "ON", "Every reading"]);
    ck.check_opts("manual/limits", &page, false);
    save1(out, "limits");

    // Record page: every reading, until stopped.
    host::reset(0);
    ui::record_page(["Every reading", "Until stopped"], "about 240 h left", false);
    ck.check_opts("manual/record", &page, false);
    save1(out, "record");

    // Temperature page: thermocouple K, no offset.
    host::reset(0);
    ui::temp_page(&ui::TempPage { sensor: 0, offs: 0.0, unit_f: false, r25: 10_000.0, beta: 3950.0, info: "Cold junction 24.6 C" });
    ck.check_opts("manual/temp", &page, false);
    save1(out, "temp");

    // Device page (no serial number on it).
    host::reset(0);
    let ver = format!("opendm40 {}", ui::FW_VERSION);
    ui::device_page(&[
        ("Vendor", "Alientek"),
        ("Web", "www.alientek.com"),
        ("Model", "DM40A"),
        ("Bootloader", "V1.1"),
        ("Software", &ver),
        ("Hardware", "V4"),
        ("Board temp", "31.5 C (MCU)"),
    ]);
    ck.check_opts("manual/device", &page, false);
    save1(out, "device");

    // Fuse check passed.
    {
        host::reset(0);
        let mut b = [0u8; 32];
        ui::fuse_screen(2, sentinel::FuseRes::Ok(48.12), &mut b);
        save1(out, "fuse-check");
    }

    // Power meter: wiring picture (default limit 6 A / 30 s) and Power
    // Settings in its idle state.
    {
        let pcs = pw_cases();
        let rc = ui::RateChip { rate: 1, period_ms: 430, applies: false };
        host::reset(0);
        power_page(pcs[0].1, 0, sentinel::Alert::None, rc);
        ui::pw::entry_dialog(6.0, 30);
        save1(out, "power-wiring");
        host::reset(0);
        let mut p = dmm::pw::Pw::new();
        p.vr = 3;
        p.job_vr = 3;
        p.coup = pcs[0].1.coup;
        ui::pw::settings_page(&dmm::pw::PowerVals { v_auto: true, i_auto: true, comp: false, ..pcs[0].1 });
        ui::pw::settings_status(&p, 0, ui::pw::HELP_COUP);
        save1(out, "power-settings");
    }

    // Wave view: mains on the 600 V range (200 V/div), 10 ms/div, triggered.
    {
        let base = &stock::scenes()[0];
        let mut m = base.m;
        m.src = scope_ui::SRC_DMM;
        m.status = scope_ui::ST_TD;
        m.tb = 18;
        m.wvdiv = 200.0;
        m.amps = false;
        m.ch1_txt = "AC V";
        m.ac = true;
        m.trig_v = 10.0;
        m.tabs = 3;
        let cells = vec!["Freq:50.0Hz", "Vp-p:651V", "Cycl:20.0ms", "Vrms:230V"];
        let sc = stock::Scene { name: "manual-wave", m, popup: scope_ui::Popup::None, trace: base.trace.clone(), persist: None, cols: 399, cells };
        host::reset(0);
        scope_ui::invalidate();
        stock::draw(&sc);
        ck.check_replica("manual/wave");
        save1(out, "wave");
    }

    for (k, (n, s)) in &ck.seen {
        println!("VIOLATION x{n} (first {s}): {k}");
    }
    println!("manual: {} screens, {} violations", ck.cases, ck.violations);
    ck.violations
}
