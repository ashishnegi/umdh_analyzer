use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, Error};
use std::path::Path;
use std::{env, io};

fn parse_umdh_file(file_name: &String) -> Result<HashMap<String, HashSet<i64>>, Error> {
    let path = Path::new(&file_name);

    // Open the path in read-only mode, returns `io::Result<File>`
    let file = File::open(&path)?;
    let lines = io::BufReader::new(file).lines();

    let mut backtrace_addresses: HashMap<String, HashSet<i64>> = HashMap::new();

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
    backtrace_map: &Vec<&HashMap<String, HashSet<i64>>>,
) -> HashMap<String, HashSet<i64>> {
    let mut common_allocations: HashMap<String, HashSet<i64>> = HashMap::new();
    // find allocations which are common in all.
    for k in all_backtraces.iter() {
        let mut present = true;
        let mut current_set = HashSet::new();
        if backtrace_map[0].contains_key(k) {
            current_set = backtrace_map[0].get(k).unwrap().clone();
        } else {
            continue;
        }

        for bk in backtrace_map.iter().skip(1) {
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

fn print_allocations(mut keys: Vec<String>, allocations_diff: &Vec<HashMap<String, HashSet<i64>>>) {
    let common_allocations = allocations_diff.last().unwrap();
    keys.sort_by(|a, b| {
        common_allocations[a]
            .len()
            .cmp(&common_allocations[b].len())
            .reverse()
    });

    println!("Common backtraces in order of highest numbers:");
    for key in keys {
        for c_a in allocations_diff {
            if let Some(allocs) = c_a.get(&key) {
                print!("{:?},", allocs.len())
            } else {
                print!(",")
            }
        }

        println!("{}", key);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        println!(
            "Usage: cargo run -- umdh_file_path1 umdh_file_path2 \n
                 File paths in order of oldest to latest."
        );
        return;
    }

    let num_files = args.len() - 1;

    let mut backtrace_map: Vec<HashMap<String, HashSet<i64>>> = Vec::new();

    for umdh_file in args.iter().skip(1) {
        backtrace_map.push(parse_umdh_file(&umdh_file).unwrap());
    }

    let mut all_backtraces_set: HashSet<String> = HashSet::new();
    for keys in backtrace_map.iter() {
        all_backtraces_set.extend(keys.keys().cloned());
    }

    let all_backtraces = all_backtraces_set.iter().cloned().collect::<Vec<String>>();

    let mut allocations_diff = Vec::new();
    for i in 0..backtrace_map.len() - 1 {
        allocations_diff.push(find_common_allocations(
            &all_backtraces,
            &vec![&backtrace_map[i], &backtrace_map[backtrace_map.len() - 1]],
        ));
    }

    if allocations_diff.len() != num_files - 1 {
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

        for c_a in allocations_diff.iter() {
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

    let leaked_allocations = find_common_allocations(
        &all_backtraces,
        &backtrace_map
            .iter()
            .collect::<Vec<&HashMap<String, HashSet<i64>>>>(),
    );

    println!("Potential Leaked allocations:");
    print_allocations(leaked_backtraces, &allocations_diff);
    println!("Potential Variable allocations:");
    print_allocations(variable_backtraces, &allocations_diff);

    println!("Allocations that never changed address:");
    let mut leaked_allocations_vec: Vec<String> =
        leaked_allocations.keys().cloned().collect::<Vec<String>>();
    leaked_allocations_vec.sort_by(|a, b| {
        leaked_allocations[a]
            .len()
            .cmp(&leaked_allocations[b].len())
            .reverse()
    });
    for k in leaked_allocations_vec {
        println!(
            "{},{} => {:?}",
            k,
            leaked_allocations[&k].len(),
            leaked_allocations[&k]
        );
    }

    println!("Static allocations:");
    print_allocations(static_backtraces, &allocations_diff);

    println!(
        "BackTraces which are definitely not leaking as they were not present in some umdh file"
    );
    println!("{:?}", missing_keys);
    println!("Allocations of last diff");
    println!("{:?}", allocations_diff.last().unwrap());
}
