use std::{collections::BTreeMap, time::Duration};

pub fn generate_file_name() -> String {
    chrono::Local::now()
        .date_naive()
        .to_string()
        .replace('-', "")
}
pub fn calculate_sum(data: &BTreeMap<String, Vec<Duration>>) -> Vec<(&str, f64)> {
    let mut result_sum = data
        .iter()
        .map(|(k, v)| (k.as_str(), v.iter().sum::<Duration>().as_secs_f64()))
        .collect::<Vec<_>>();
    result_sum.sort_unstable_by(|a, b| b.1.total_cmp(&a.1));
    result_sum
}

pub fn calculate_avg(
    data: &BTreeMap<String, Vec<Duration>>,
    file_count: usize,
) -> Vec<(&str, f64)> {
    let mut result_avg = data
        .iter()
        .map(|(k, v)| {
            (
                k.as_str(),
                v.iter().sum::<Duration>().as_secs_f64() / file_count as f64,
            )
        })
        .collect::<Vec<_>>();
    result_avg.sort_unstable_by(|a, b| b.1.total_cmp(&a.1));
    result_avg
}

pub fn calculate_median(data: &BTreeMap<String, Vec<Duration>>) -> Vec<(&str, f64)> {
    let mut result_median = data
        .iter()
        .map(|(k, v)| {
            let mut v = v.clone();
            v.sort();
            (
                k.as_str(),
                if v.len() % 2 == 0 {
                    let middle = v.len() / 2;
                    if middle >= 1 {
                        (v[middle - 1] + v[middle]).as_secs_f64() / 2.
                    } else {
                        0.
                    }
                } else if !v.is_empty() {
                    v[v.len() / 2].as_secs_f64()
                } else {
                    0.
                },
            )
        })
        .collect::<Vec<_>>();
    result_median.sort_unstable_by(|a, b| b.1.total_cmp(&a.1));
    result_median
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_calculate_median_with_no_data() {
        let data: BTreeMap<String, Vec<Duration>> = BTreeMap::new();
        let vec: Vec<(&str, f64)> = vec![];
        assert_eq!(vec, calculate_median(&data));
    }

    #[test]
    fn should_calculate_median_with_single_data_odd() {
        let mut data: BTreeMap<String, Vec<Duration>> = BTreeMap::new();
        data.insert(
            "time_back".to_string(),
            vec![
                Duration::from_secs(1),
                Duration::from_secs(15),
                Duration::from_secs(40),
                Duration::from_secs(2),
                Duration::from_secs(4),
            ],
        );
        let vec = vec![("time_back", 4.0)];
        assert_eq!(vec, calculate_median(&data));
    }

    #[test]
    fn should_calculate_median_with_single_data_even() {
        let mut data: BTreeMap<String, Vec<Duration>> = BTreeMap::new();
        data.insert(
            "time_back".to_string(),
            vec![
                Duration::from_secs(5),
                Duration::from_secs(1),
                Duration::from_secs(15),
                Duration::from_secs(40),
                Duration::from_secs(2),
                Duration::from_secs(4),
            ],
        );
        let vec = vec![("time_back", 4.5)];
        assert_eq!(vec, calculate_median(&data));
    }

    #[test]
    fn should_calculate_median_with_multiple_odd_data() {
        let mut data: BTreeMap<String, Vec<Duration>> = BTreeMap::new();
        data.insert(
            "time_back".to_string(),
            vec![
                Duration::from_secs(5),
                Duration::from_secs(1),
                Duration::from_secs(15),
                Duration::from_secs(40),
                Duration::from_secs(2),
                Duration::from_secs(4),
            ],
        );
        data.insert(
            "something-else".to_string(),
            vec![
                Duration::from_secs(4),
                Duration::from_secs(10),
                Duration::from_secs(1),
                Duration::from_secs(20),
                Duration::from_secs(41),
            ],
        );
        let vec = vec![("something-else", 10.), ("time_back", 4.5)];
        assert_eq!(vec, calculate_median(&data));
    }
}
