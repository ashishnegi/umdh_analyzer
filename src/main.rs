use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, Error};
use std::path::Path;
use std::{env, io};

type BacktraceAllocationsMap = HashMap<String, HashSet<i64>>;
type BacktraceRefAllocationsMap<'a> = HashMap<&'a String, HashSet<i64>>;

fn parse_umdh_file(file_name: &str) -> Result<BacktraceAllocationsMap, Error> {
    let path = Path::new(&file_name);

    // Open the path in read-only mode, returns `io::Result<File>`
    let file = File::open(&path)?;
    let lines = io::BufReader::new(file).lines();

    let mut backtrace_addresses: BacktraceAllocationsMap = HashMap::new();

    for line_res in lines {
        let line = line_res?;
        // Format of line:
        // 30 bytes + 30 at 1E30F1B0770 by BackTraceF076263
        if line.contains("BackTrace") {
            let at_pos = line.find("at ");
            if at_pos.is_none() {
                continue;
            }

            let address_start_pos = at_pos.unwrap() + 3;
            let line_str = line.as_bytes();
            let mut address_end_pos = address_start_pos;
            while address_end_pos < line.len() && line_str[address_end_pos] != b' ' {
                address_end_pos += 1;
            }

            if address_end_pos + 4 + 9  > line.len() {
                // 4 for " by ", 9 for "BackTrace "
                continue; // bad format line ?
            }

            let address = match i64::from_str_radix(&line[address_start_pos..address_end_pos], 16) {
                Ok(address) => address,
                Err(_) => continue,
            };

            let backtrace_pos = address_end_pos + 4; //4 == " by ".len();
            let backtrace = String::from(&line[backtrace_pos..line.len()]);

            backtrace_addresses
                .entry(backtrace)
                .or_insert_with(HashSet::new)
                .insert(address);
        }
    }

    Ok(backtrace_addresses)
}

fn find_common_allocations<'a>(
    all_backtraces: &'a [&String],
    backtrace_maps: &[&'a BacktraceAllocationsMap],
) -> BacktraceRefAllocationsMap<'a> {
    let mut common_allocations: BacktraceRefAllocationsMap = HashMap::new();
    // find allocations which are common in all.
    for k in all_backtraces.iter() {
        let mut present = true;

        // Is this BackTrace present in all log files ? if not, skip costly set intersections
        for bk in backtrace_maps.iter() {
            if !bk.contains_key(*k) {
                present = false;
                break;
            }
        }

        if !present {
            continue;
        }

        let mut current_set = backtrace_maps[0].get(*k).unwrap()
            .intersection(backtrace_maps[1].get(*k).unwrap()).cloned().collect::<HashSet<i64>>();

        for bk in backtrace_maps.iter().skip(2) {
            // there is no easy (right?) way to iterate over HashSet and also remove from it;
            current_set = bk[*k]
                .intersection(&current_set)
                .cloned()
                .collect::<HashSet<i64>>();

            if current_set.is_empty() {
                present = false;
                break;
            }
        }

        if present {
            common_allocations.insert(k, current_set);
        }
    }
    common_allocations
}

fn print_allocations(backtraces: &mut Vec<&String>, allocation_diffs: &[BacktraceRefAllocationsMap]) {
    let common_allocations = allocation_diffs.last().unwrap();
    backtraces.sort_by(|a, b| {
        let a_present = common_allocations.contains_key(*a);
        let b_present = common_allocations.contains_key(*b);
        if a_present && b_present {
            // this allocation type is present in last dump
            common_allocations[*a]
            .len()
            .cmp(&common_allocations[*b].len())
            .reverse()
        } else if !a_present && !b_present {
            // both are not present : sort by name itself.
            a.cmp(b).reverse()
        } else if a_present {
            Ordering::Greater
        } else {
            Ordering::Less
        }
    });

    println!("Common allocations: [1st..Last],[2nd..Last],[3rd..Last],..,BackTrace*");
    for backtrace in backtraces {
        for c_a in allocation_diffs {
            if let Some(allocs) = c_a.get(*backtrace) {
                print!("{:?},", allocs.len())
            } else {
                print!(",")
            }
        }

        println!("{}", backtrace);
    }
}

fn parse_umdh_files(file_names: &[String]) -> Vec<BacktraceAllocationsMap> {
    let mut backtrace_maps: Vec<BacktraceAllocationsMap> = Vec::new();

    for umdh_file in file_names {
        backtrace_maps.push(parse_umdh_file(umdh_file).unwrap());
    }

    backtrace_maps
}

fn get_all_backtraces(backtrace_maps: &[BacktraceAllocationsMap]) -> Vec<&String> {
    let mut all_backtraces_set: HashSet<&String> = HashSet::new();
    for keys in backtrace_maps.iter() {
        all_backtraces_set.extend(keys.keys());
    }

    // i believe this cloned() is not costly as it is converting from &&String to &String.
    all_backtraces_set.iter().cloned().collect::<Vec<&String>>()
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
        .map(|m| find_common_allocations(&all_backtraces, &[m, backtrace_maps.last().unwrap()]))
        .collect::<Vec<BacktraceRefAllocationsMap>>();

    if allocation_diffs.len() != num_files - 1 {
        panic!("unexpected allocation diff count")
    }

    // strictly increasing common allocation counts.
    let mut leaked_backtraces: Vec<&String> = Vec::new();
    // constant common allocation counts.
    let mut static_backtraces: Vec<&String> = Vec::new();
    // variable allocations - increasing & decreasing with time.
    let mut variable_backtraces: Vec<&String> = Vec::new();

    let mut missing_keys: HashMap<&String, usize> = HashMap::new();
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
            missing_keys.insert(k, last_count);
            continue;
        }

        if is_variable {
            variable_backtraces.push(k);
        } else if is_static {
            static_backtraces.push(k);
        } else {
            leaked_backtraces.push(k);
        }
    }

    println!("Potential Leaked allocations as these allocations always kept increasing:");
    print_allocations(&mut leaked_backtraces, &allocation_diffs);

    println!("Variable allocations: [Count increased and decreased with time] / Can be waiting on some workflow like GC to deallocate these");
    print_allocations(&mut variable_backtraces, &allocation_diffs);

    println!("Allocations that never changed address: Sorted by count: [These can be global or leaked allocations]");
    let always_present_allocations = find_common_allocations(
        &all_backtraces,
        &backtrace_maps.iter().collect::<Vec<&BacktraceAllocationsMap>>(),
    );
    let mut always_present_allocations_vec = always_present_allocations
        .keys()
        .collect::<Vec<&&String>>();
    always_present_allocations_vec.sort_by(|a, b| {
        always_present_allocations[**a]
            .len()
            .cmp(&always_present_allocations[**b].len())
            .reverse()
    });
    for k in always_present_allocations_vec {
        if always_present_allocations[k].len() > 1 {
            println!(
                "{},{} => {:?}",
                k,
                always_present_allocations[k].len(),
                always_present_allocations[k]
            );
        }
    }

    println!("Static allocations: [Count never changed]");
    print_allocations(&mut static_backtraces, &allocation_diffs);

    println!(
        "BackTraces which are definitely not leaking as they were not present in some umdh file"
    );
    println!("{:?}", missing_keys);
    println!("Allocations of last diff");
    println!("{:?}", allocation_diffs.last().unwrap());
}
