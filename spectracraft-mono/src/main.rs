use spectracraft_mono::{analyze_with_texture, load_wav, render_wav, Model, Result};
use std::path::Path;
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
            let profile=if action=="analyze-flute"{spectracraft_mono::harmonic::Profile::FluteReference}else{Default::default()};
            let packet=spectracraft_mono::harmonic::analyze(&wave,rate,profile)?;
            spectracraft_mono::qa::write_new(Path::new(output),&serde_json::to_vec_pretty(&packet)?)?;
        }
        [action,input,output] if action=="render-harmonic" => {
            let file=std::fs::File::open(input)?;
            if file.metadata()?.len()>64*1024*1024{return Err("Packet too large".into());}
            let packet=serde_json::from_reader(std::io::BufReader::new(file))?;
            let samples=spectracraft_mono::harmonic::render(&packet,734,false,false)?.mix();
            spectracraft_mono::write_wav(packet.rate,&samples,Path::new(output))?;
        }
        [action,input,output] if action=="analyze-layers" || action=="analyze-layers-faint" => {
            let (rate,wave)=load_wav(Path::new(input))?;
            let packet=spectracraft_mono::edge::analyze(&wave,rate,spectracraft_mono::edge::Options{faint:action=="analyze-layers-faint",..Default::default()})?;
            spectracraft_mono::qa::write_new(Path::new(output),&serde_json::to_vec_pretty(&packet)?)?;
        }
        [action,input,output] if action=="render-layers" => {
            let file=std::fs::File::open(input)?;
            if file.metadata()?.len()>64*1024*1024 {return Err("Layer packet too large".into());}
            let packet:spectracraft_mono::edge::Packet=serde_json::from_reader(std::io::BufReader::new(file))?;
            let rendered=spectracraft_mono::workspace::render(&packet,734)?;
            let gain=rendered.headroom(&[]);
            std::fs::create_dir(output)?;
            for (name,stem) in ["pitch","ring","texture","accent"].iter().zip(&rendered.stems) {
                let samples:Vec<_>=stem.iter().map(|v|v*gain).collect();
                spectracraft_mono::write_wav(packet.base.rate,&samples,&Path::new(output).join(format!("{name}.wav")))?;
            }
            let mix:Vec<_>=rendered.mix(&Default::default())?.into_iter().map(|v|v*gain).collect();
            spectracraft_mono::write_wav(packet.base.rate,&mix,&Path::new(output).join("mix.wav"))?;
            spectracraft_mono::qa::write_new(&Path::new(output).join("export.json"),&serde_json::to_vec_pretty(&serde_json::json!({"common_gain":gain,"seed":734}))?)?;
        }
        [action,input,output,start,end] if action=="repair-gap" => {
            spectracraft_mono::gap::repair_wav(Path::new(input),Path::new(output),start.to_string_lossy().parse()?,end.to_string_lossy().parse()?)?;
        }
        [action,input,output] if action=="analyze" || action=="analyze-texture" => {
            let (rate,wave)=load_wav(Path::new(input))?; let model=analyze_with_texture(&wave,rate,action=="analyze-texture")?;
            spectracraft_mono::qa::write_new(Path::new(output),&serde_json::to_vec(&model)?)?;
        }
        [action,input,output] if action=="measure-attacks" => {
            let (rate,wave)=load_wav(Path::new(input))?;
            let evidence=spectracraft_mono::attack_guard::measure_quiet_attacks(&wave,rate)?;
            spectracraft_mono::qa::write_new(Path::new(output),&serde_json::to_vec(&evidence)?)?;
        }
        [action,input,output] if action=="analyze-guitar-legacy" => {
            let (spec,wave)=spectracraft_mono::qa::read_audio(Path::new(input))?;
            if spec.channels!=1 {return Err("Expected mono guitar".into());}
            let rate=spec.sample_rate;
            let cues=spectracraft_mono::reperform::analyze_guitar_legacy(&wave,rate)?;
            spectracraft_mono::qa::write_new(Path::new(output),&serde_json::to_vec_pretty(&cues)?)?;
        }
        [action,input,output] if action=="analyze-guitar" || action=="rebuild-dry-guitar" => {
            let performance=spectracraft_mono::reperform::analyze_guitar_wav(Path::new(input))?;
            if action=="rebuild-dry-guitar" {
                performance.write_dry_wav(Path::new(output))?;
            } else {
                spectracraft_mono::qa::write_new(Path::new(output),&serde_json::to_vec(&performance)?)?;
            }
        }
        [action,input,output] if action=="render-dry-guitar" => {
            use spectracraft_mono::reperform::Performance;
            let file=std::fs::File::open(input)?;
            if file.metadata()?.len()>16*1024*1024 { return Err("Cue file is too large".into()); }
            let performance: Performance=serde_json::from_reader(std::io::BufReader::new(file))?;
            performance.write_dry_wav(Path::new(output))?;
        }
        [action,input,source_idx,output] if action=="render-dry-source" => {
            use spectracraft_mono::{reperform::Performance, studio_catalog};
            let file=std::fs::File::open(input)?;
            if file.metadata()?.len()>16*1024*1024 { return Err("Cue file is too large".into()); }
            let performance: Performance=serde_json::from_reader(std::io::BufReader::new(file))?;
            let sources=studio_catalog::source_catalog();
            let idx: usize = source_idx.to_string_lossy().parse()?;
            let src=&sources[idx.min(sources.len().saturating_sub(1))];
            performance.write_dry_wav_for_source(src, Path::new(output))?;
        }
        [action,input,output] if action=="render" => {
            let f=std::fs::File::open(input)?;
            if f.metadata()?.len()>32*1024*1024 { return Err("Feature file is too large".into()); }
            let model: Model=serde_json::from_reader(std::io::BufReader::new(f))?;
            let gain=render_wav(&model,12345,Path::new(output))?;
            println!("Export gain: {gain:.6}; render seed: 12345");
        }
        [action,input,cues,output,selection] if action=="export-guitar-variants" => {
            let report=spectracraft_mono::qa::export_guitar_variants(
                Path::new(input),
                Path::new(cues),
                Path::new(output),
                &selection.to_string_lossy(),
                spectracraft_mono::capture::DryReviewStatus::NotReviewed,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        [action,input,cues,output,selection,flag] if action=="export-guitar-variants" && flag=="--human-approved" => {
            let report=spectracraft_mono::qa::export_guitar_variants(
                Path::new(input),
                Path::new(cues),
                Path::new(output),
                &selection.to_string_lossy(),
                spectracraft_mono::capture::DryReviewStatus::ApprovedByHuman,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        [action,input,output,room,mic,x,y,z] if action=="capture" || action=="capture-tail" => {
            use spectracraft_mono::capture::{render,predict,Room,Mic,Setup};
            let room=match room.to_str() {Some("direct")=>Room::Direct,Some("studio")=>Room::Studio,Some("hall")=>Room::Hall,Some("booth")=>Room::Booth,Some("chamber")=>Room::Chamber,Some("open")=>Room::Open,_=>return Err("Expected direct, booth, studio, chamber, hall or open".into())};
            let mic=mic.to_str().and_then(Mic::parse).ok_or("Expected omni, wide, cardioid, super or figure8")?;
            let mut offset=[0.;3];
            for (t,v) in offset.iter_mut().zip([x,y,z]) { *t=v.to_string_lossy().parse()?; }
            let setup=Setup{room,mic,offset,..Setup::default()};
            let (rate,source)=load_wav(Path::new(input))?;
            let y=if action=="capture-tail" { spectracraft_mono::capture::render_with_tail(&source,rate,setup) } else { render(&source,rate,setup) };
            let gain=spectracraft_mono::write_wav(rate,&y,Path::new(output))?;
            println!("{:?}; WAV export gain {gain}",predict(setup));
        }
        _ => return Err("Usage: spectracraft-mono analyze-layers|analyze-layers-faint INPUT.wav PACKET.json | render-layers PACKET.json NEW_DIR | analyze-harmonic|analyze-flute INPUT.wav PACKET.json | render-harmonic PACKET.json OUTPUT.wav | analyze-guitar-legacy INPUT.wav CUES.json | analyze|analyze-texture INPUT.wav FEATURES.json | render FEATURES.json OUTPUT.wav | analyze-guitar INPUT.wav CUES.json | render-dry-guitar CUES.json OUTPUT.wav | rebuild-dry-guitar INPUT.wav OUTPUT.wav | export-guitar-variants INPUT.wav CUES.json NEW_DIR all|VARIANT[,VARIANT...] | measure-attacks INPUT.wav ATTACKS.json | repair-gap INPUT.wav OUTPUT.wav START_FRAME END_FRAME | capture|capture-tail INPUT.wav OUTPUT.wav ROOM MIC X Y Z".into()),
    }
    Ok(())
}
