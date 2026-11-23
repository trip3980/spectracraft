use spectracraft_mono::capture::{predict, render, render_with_tail, Room, Setup};
#[test]
fn capture_tail_extends_export_and_preserves_the_existing_prefix() {
    for rate in [8000, 44100, 48000, 96000] {
        let mut x = vec![0.; 128];
        x[127] = 0.5;
        let setup = Setup::default();
        let short = render(&x, rate, setup);
        let long = render_with_tail(&x, rate, setup);
        assert_eq!(long[..x.len()], short);
        assert!(long.len() > x.len());
        assert!(long[x.len()..].iter().any(|v| v.abs() > 0.001));
        assert!(
            long.len() - x.len()
                <= ((predict(setup).last_path_ms * rate as f32 / 1000.).ceil() as usize + 2)
        );
        let dry = Setup {
            room: Room::Direct,
            ..setup
        };
        assert_eq!(render_with_tail(&x, rate, dry), x);
    }
}
