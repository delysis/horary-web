//! Losslessly convert one whole-guide proposal to a short exact edit. This is
//! normalization, not a new teacher proposal; prove equal applied guide hashes.
#![forbid(unsafe_code)]
use horary_prompt_program::{digest, Edit, Program, Signature};
use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    if arguments.len() != 3 {
        return Err("usage: compact candidate.json original-guide.txt fresh-output.json".into());
    }
    let candidate_bytes = fs::read(&arguments[0])?;
    let mut program = Program::parse(&candidate_bytes)?;
    if program.overrides.len() != 1 || !program.overrides[0].edits.is_empty() {
        return Err("This normalization requires one full-guide proposal".into());
    }
    let guide = fs::read_to_string(&arguments[1])?;
    let replacement = program.overrides[0].replacement_text.clone();
    let scope = &program.overrides[0];
    let signature = Signature {
        stage: &scope.stage,
        recognition_phase: scope.recognition_phase.as_deref(),
        method: scope.method.as_deref(),
    };
    let prior = program
        .apply(signature, &guide)?
        .ok_or("Proposal did not apply")?
        .0;
    let prefix = guide
        .chars()
        .zip(replacement.chars())
        .take_while(|(a, b)| a == b)
        .map(|(character, _)| character.len_utf8())
        .sum::<usize>();
    let mut suffix = 0;
    for (a, b) in guide[prefix..]
        .chars()
        .rev()
        .zip(replacement[prefix..].chars().rev())
    {
        if a != b {
            break;
        }
        suffix += a.len_utf8();
    }
    let mut begin = prefix.saturating_sub(32);
    while !guide.is_char_boundary(begin) {
        begin -= 1;
    }
    let end = guide.len() - suffix;
    let next_end = replacement.len() - suffix;
    let edit = Edit {
        old_text: guide[begin..end].into(),
        new_text: replacement[begin..next_end].into(),
    };
    program.overrides[0].replacement_text.clear();
    program.overrides[0].edits = vec![edit];
    program.validate()?;
    let scope = &program.overrides[0];
    let after = program
        .apply(
            Signature {
                stage: &scope.stage,
                recognition_phase: scope.recognition_phase.as_deref(),
                method: scope.method.as_deref(),
            },
            &guide,
        )?
        .ok_or("Compact proposal did not apply")?
        .0;
    if prior != after {
        return Err("Normalization changed teaching".into());
    }
    let bytes = serde_json::to_vec_pretty(&program)?;
    let output = PathBuf::from(&arguments[2]);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)?;
    use std::io::Write;
    file.write_all(&bytes)?;
    file.sync_all()?;
    let receipt = serde_json::json!({"operation":"lossless whole-guide to exact-edit normalization", "original_candidate_sha256":digest(&candidate_bytes),"normalized_candidate_sha256":digest(&bytes),"applied_guide_sha256":digest(prior),"source_quotations_unchanged":true});
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.with_extension("normalization.json"))?;
    file.write_all(&serde_json::to_vec_pretty(&receipt)?)?;
    file.sync_all()?;
    println!("{}", receipt);
    Ok(())
}
