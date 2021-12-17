use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, Error};
use std::path::Path;
use std::{env, io};

type BackTraceMap = HashMap<String, HashSet<i64>>;

fn parse_umdh_file(file_name: &String) -> Result<BackTraceMap, Error> {
    let path = Path::new(&file_name);

    // Open the path in read-only mode, returns `io::Result<File>`
    let file = File::open(&path)?;
    let lines = io::BufReader::new(file).lines();

    let mut backtrace_addresses: BackTraceMap = HashMap::new();

    for op_line in lines {
        let line = op_line?;
        if line.contains("BackTrace") {
            let at_pos = line.find("at ");
            if at_pos.is_none() {
                continue;
            }

            let address_pos = at_pos.unwrap() + 3;
            // would have liked no allocation
            let address_str: String = line
                .chars()
                .skip(address_pos)
                .take_while(|c| *c != ' ')
                .collect();

            let address = match i64::from_str_radix(&address_str, 16) {
                Ok(address) => address,
                Err(_) => continue,
            };

            let backtrace_pos = address_pos + address_str.len() + " by ".len();
            let backtrace = String::from(&line[backtrace_pos..line.len()]);

            backtrace_addresses
                .entry(backtrace)
                .or_insert(HashSet::new())
                .insert(address);
        }
    }

    Ok(backtrace_addresses)
}

fn find_common_allocations(
    all_backtraces: &Vec<String>,
    backtrace_maps: &Vec<&BackTraceMap>,
) -> BackTraceMap {
    let mut common_allocations: BackTraceMap = HashMap::new();
    // find allocations which are common in all.
    for k in all_backtraces.iter() {
        let mut present = true;
        let mut current_set = HashSet::new();
        if backtrace_maps[0].contains_key(k) {
            current_set = backtrace_maps[0].get(k).unwrap().clone();
        } else {
            continue;
        }

        for bk in backtrace_maps.iter().skip(1) {
            if !bk.contains_key(k) {
                present = false;
                break;
            }

            current_set = bk[k]
                .intersection(&current_set)
                .cloned()
                .collect::<HashSet<i64>>();

            if current_set.len() == 0 {
                present = false;
                break;
            }
        }

        if present {
            common_allocations.insert(k.clone(), current_set);
        }
    }
    common_allocations
}

fn print_allocations(mut keys: Vec<String>, allocation_diffs: &Vec<BackTraceMap>) {
    let common_allocations = allocation_diffs.last().unwrap();
    keys.sort_by(|a, b| {
        common_allocations[a]
            .len()
            .cmp(&common_allocations[b].len())
            .reverse()
    });

    println!("Common allocations: [1st..Last],[2nd..Last],[3rd..Last],..,BackTrace*");
    for key in keys {
        for c_a in allocation_diffs {
            if let Some(allocs) = c_a.get(&key) {
                print!("{:?},", allocs.len())
            } else {
                print!(",")
            }
        }

        println!("{}", key);
    }
}

fn parse_umdh_files(file_names: &[String]) -> Vec<BackTraceMap> {
    let mut backtrace_maps: Vec<BackTraceMap> = Vec::new();

    for umdh_file in file_names {
        backtrace_maps.push(parse_umdh_file(&umdh_file).unwrap());
    }

    backtrace_maps
}

fn get_all_backtraces(backtrace_maps: &Vec<BackTraceMap>) -> Vec<String> {
    let mut all_backtraces_set: HashSet<String> = HashSet::new();
    for keys in backtrace_maps.iter() {
        all_backtraces_set.extend(keys.keys().cloned());
    }

    all_backtraces_set.iter().cloned().collect::<Vec<String>>()
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        println!(
            "Usage: cargo run -- umdh_file_path1 umdh_file_path2  umdh_file_path3\n
                 File paths in order of oldest to latest."
        );
        return;
    }

    let num_files = args.len() - 1;

    let backtrace_maps = parse_umdh_files(&args[1..]);
    let all_backtraces = get_all_backtraces(&backtrace_maps);
    let allocation_diffs = backtrace_maps
        .iter()
        .take(backtrace_maps.len() - 1)
        .map(|m| find_common_allocations(&all_backtraces, &vec![m, backtrace_maps.last().unwrap()]))
        .collect::<Vec<BackTraceMap>>();

    if allocation_diffs.len() != num_files - 1 {
        panic!("unexpected allocation diff count")
    }

    // strictly increasing common allocation counts.
    let mut leaked_backtraces = Vec::new();
    // constant common allocation counts.
    let mut static_backtraces = Vec::new();
    // variable allocations - with no common pattern.
    let mut variable_backtraces = Vec::new();

    let mut missing_keys: HashMap<String, usize> = HashMap::new();
    // get allocation in differet buckets.
    for k in all_backtraces.iter() {
        let mut last_count = 0;
        let mut is_variable = false;
        let mut is_static = true;
        let mut not_present = false;

        for c_a in allocation_diffs.iter() {
            if let Some(allocs) = c_a.get(k) {
                if allocs.len() >= last_count {
                    if (last_count != 0) && (allocs.len() != last_count) {
                        is_static = false;
                    }
                    last_count = allocs.len();
                } else {
                    is_variable = true;
                }
            } else {
                not_present = true;
            }
        }

        // trace only if present in only few files
        if not_present {
            missing_keys.insert(k.clone(), last_count);
            continue;
        }

        if is_variable {
            variable_backtraces.push(k.clone());
        } else if is_static {
            static_backtraces.push(k.clone());
        } else {
            leaked_backtraces.push(k.clone());
        }
    }

    println!("Potential Leaked allocations as these allocations always kept increasing:");
    print_allocations(leaked_backtraces, &allocation_diffs);

    println!("Variable allocations: [Count increased and decreased with time] / Can be waiting on some workflow like GC to deallocate these");
    print_allocations(variable_backtraces, &allocation_diffs);

    println!("Allocations that never changed address: Sorted by count: [These can be global or leaked allocations]");
    let always_present_allocations = find_common_allocations(
        &all_backtraces,
        &backtrace_maps.iter().collect::<Vec<&BackTraceMap>>(),
    );
    let mut always_present_allocations_vec: Vec<String> = always_present_allocations
        .keys()
        .cloned()
        .collect::<Vec<String>>();
    always_present_allocations_vec.sort_by(|a, b| {
        always_present_allocations[a]
            .len()
            .cmp(&always_present_allocations[b].len())
            .reverse()
    });
    for k in always_present_allocations_vec {
        if always_present_allocations[&k].len() > 1 {
            println!(
                "{},{} => {:?}",
                k,
                always_present_allocations[&k].len(),
                always_present_allocations[&k]
            );
        }
    }

    println!("Static allocations: [Count never changed]");
    print_allocations(static_backtraces, &allocation_diffs);

    println!(
        "BackTraces which are definitely not leaking as they were not present in some umdh file"
    );
    println!("{:?}", missing_keys);
    println!("Allocations of last diff");
    println!("{:?}", allocation_diffs.last().unwrap());
}
