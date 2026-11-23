use spectral_canvas_mono::{analyze_with_texture, load_wav, render_wav, Model, Result};
use std::{fs::OpenOptions, io::Write, path::Path};
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [action,input,output] if action=="analyze-harmonic" || action=="analyze-flute" => {
            let(rate,wave)=load_wav(Path::new(input))?;
            let profile=if action=="analyze-flute"{spectral_canvas_mono::harmonic::Profile::FluteReference}else{Default::default()};
            let packet=spectral_canvas_mono::harmonic::analyze(&wave,rate,profile)?;
            spectral_canvas_mono::qa::write_new(Path::new(output),&serde_json::to_vec_pretty(&packet)?)?;
        }
        [action,input,output] if action=="render-harmonic" => {
            let file=std::fs::File::open(input)?;
            if file.metadata()?.len()>64*1024*1024{return Err("Packet too large".into());}
            let packet=serde_json::from_reader(std::io::BufReader::new(file))?;
            let samples=spectral_canvas_mono::harmonic::render(&packet,734,false,false)?.mix();
            spectral_canvas_mono::write_wav(packet.rate,&samples,Path::new(output))?;
        }
        [action,input,output] if action=="analyze-layers" || action=="analyze-layers-faint" => {
            let (rate,wave)=load_wav(Path::new(input))?;
            let packet=spectral_canvas_mono::edge::analyze(&wave,rate,spectral_canvas_mono::edge::Options{faint:action=="analyze-layers-faint",..Default::default()})?;
            spectral_canvas_mono::qa::write_new(Path::new(output),&serde_json::to_vec_pretty(&packet)?)?;
        }
        [action,input,output] if action=="render-layers" => {
            let file=std::fs::File::open(input)?;
            if file.metadata()?.len()>64*1024*1024 {return Err("Layer packet too large".into());}
            let packet:spectral_canvas_mono::edge::Packet=serde_json::from_reader(std::io::BufReader::new(file))?;
            let rendered=spectral_canvas_mono::workspace::render(&packet,734)?;
            let gain=rendered.headroom(&[]);
            std::fs::create_dir(output)?;
            for (name,stem) in ["pitch","ring","texture","accent"].iter().zip(&rendered.stems) {
                let samples:Vec<_>=stem.iter().map(|v|v*gain).collect();
                spectral_canvas_mono::write_wav(packet.base.rate,&samples,&Path::new(output).join(format!("{name}.wav")))?;
            }
            let mix:Vec<_>=rendered.mix(&Default::default())?.into_iter().map(|v|v*gain).collect();
            spectral_canvas_mono::write_wav(packet.base.rate,&mix,&Path::new(output).join("mix.wav"))?;
            spectral_canvas_mono::qa::write_new(&Path::new(output).join("export.json"),&serde_json::to_vec_pretty(&serde_json::json!({"common_gain":gain,"seed":734}))?)?;
        }
        [action,input,output,start,end] if action=="repair-gap" => {
            spectral_canvas_mono::gap::repair_wav(Path::new(input),Path::new(output),start.to_string_lossy().parse()?,end.to_string_lossy().parse()?)?;
        }
        [action,input,output] if action=="analyze" || action=="analyze-texture" => {
            let (rate,wave)=load_wav(Path::new(input))?; let model=analyze_with_texture(&wave,rate,action=="analyze-texture")?;
            let f=OpenOptions::new().write(true).create_new(true).open(output)?;
            let mut writer = std::io::BufWriter::new(f);
            serde_json::to_writer(&mut writer,&model)?;
            writer.flush()?;
        }
        [action,input,output] if action=="measure-attacks" => {
            let (rate,wave)=load_wav(Path::new(input))?;
            let evidence=spectral_canvas_mono::attack_guard::measure_quiet_attacks(&wave,rate)?;
            let f=OpenOptions::new().write(true).create_new(true).open(output)?;
            let mut writer=std::io::BufWriter::new(f);
            serde_json::to_writer(&mut writer,&evidence)?;
            writer.flush()?;
        }
        [action,input,output] if action=="analyze-guitar-legacy" => {
            let (spec,wave)=spectral_canvas_mono::qa::read_audio(Path::new(input))?;
            if spec.channels!=1 {return Err("Expected mono guitar".into());}
            let rate=spec.sample_rate;
            let cues=spectral_canvas_mono::reperform::analyze_guitar_legacy(&wave,rate)?;
            spectral_canvas_mono::qa::write_new(Path::new(output),&serde_json::to_vec_pretty(&cues)?)?;
        }
        [action,input,output] if action=="analyze-guitar" || action=="rebuild-dry-guitar" => {
            let performance=spectral_canvas_mono::reperform::analyze_guitar_wav(Path::new(input))?;
            if action=="rebuild-dry-guitar" {
                performance.write_dry_wav(Path::new(output))?;
            } else {
                let f=OpenOptions::new().write(true).create_new(true).open(output)?;
                let mut writer=std::io::BufWriter::new(f);
                serde_json::to_writer(&mut writer,&performance)?;
                writer.flush()?;
            }
        }
        [action,input,output] if action=="render-dry-guitar" => {
            use spectral_canvas_mono::reperform::Performance;
            let file=std::fs::File::open(input)?;
            if file.metadata()?.len()>16*1024*1024 { return Err("Cue file is too large".into()); }
            let performance: Performance=serde_json::from_reader(std::io::BufReader::new(file))?;
            performance.write_dry_wav(Path::new(output))?;
        }
        [action,input,output] if action=="render" => {
            let f=std::fs::File::open(input)?;
            if f.metadata()?.len()>32*1024*1024 { return Err("Feature file is too large".into()); }
            let model: Model=serde_json::from_reader(std::io::BufReader::new(f))?;
            let gain=render_wav(&model,12345,Path::new(output))?;
            println!("Export gain: {gain:.6}; render seed: 12345");
        }
        [action,input,output,room,mic,x,y,z] if action=="capture" => {
            use spectral_canvas_mono::capture::{render,predict,Room,Mic,Setup};
            let room=match room.to_str() {Some("direct")=>Room::Direct,Some("studio")=>Room::Studio,Some("hall")=>Room::Hall,Some("booth")=>Room::Booth,Some("chamber")=>Room::Chamber,Some("open")=>Room::Open,_=>return Err("Expected direct, booth, studio, chamber, hall or open".into())};
            let mic=mic.to_str().and_then(Mic::parse).ok_or("Expected omni, wide, cardioid, super or figure8")?;
            let mut offset=[0.;3];
            for (t,v) in offset.iter_mut().zip([x,y,z]) { *t=v.to_string_lossy().parse()?; }
            let setup=Setup{room,mic,offset,..Setup::default()};
            let (rate,source)=load_wav(Path::new(input))?;
            let y=if action=="capture-tail" { spectral_canvas_mono::capture::render_with_tail(&source,rate,setup) } else { render(&source,rate,setup) };
            let gain=spectral_canvas_mono::write_wav(rate,&y,Path::new(output))?;
            println!("{:?}; WAV export gain {gain}",predict(setup));
        }
        _ => return Err("Usage: spectral-canvas-mono analyze-layers|analyze-layers-faint INPUT.wav PACKET.json | render-layers PACKET.json NEW_DIR | analyze-harmonic|analyze-flute INPUT.wav PACKET.json | render-harmonic PACKET.json OUTPUT.wav | analyze-guitar-legacy INPUT.wav CUES.json | analyze|analyze-texture INPUT.wav FEATURES.json | render FEATURES.json OUTPUT.wav | analyze-guitar INPUT.wav CUES.json | render-dry-guitar CUES.json OUTPUT.wav | rebuild-dry-guitar INPUT.wav OUTPUT.wav | measure-attacks INPUT.wav ATTACKS.json | repair-gap INPUT.wav OUTPUT.wav START_FRAME END_FRAME | capture|capture-tail INPUT.wav OUTPUT.wav ROOM MIC X Y Z".into()),
    }
    Ok(())
}
