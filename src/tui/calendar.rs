use chrono::{Datelike, Duration, NaiveDate, Weekday};

pub fn month_start(date: NaiveDate) -> NaiveDate {
    date.with_day(1).expect("valid first day of month")
}

pub fn shift_month(date: NaiveDate, months: i32) -> NaiveDate {
    let base = month_start(date);
    let total_months = base.year() * 12 + base.month0() as i32 + months;
    let year = total_months.div_euclid(12);
    let month0 = total_months.rem_euclid(12) as u32;
    NaiveDate::from_ymd_opt(year, month0 + 1, 1).expect("valid shifted month")
}

pub fn shift_date_by_months(date: NaiveDate, months: i32) -> NaiveDate {
    let target_month = shift_month(date, months);
    let day = date
        .day()
        .min(days_in_month(target_month.year(), target_month.month()));
    NaiveDate::from_ymd_opt(target_month.year(), target_month.month(), day)
        .expect("valid shifted date")
}

pub fn month_grid(date: NaiveDate) -> Vec<Vec<NaiveDate>> {
    let first = month_start(date);
    let start = first - Duration::days(first.weekday().num_days_from_sunday() as i64);
    (0..6)
        .map(|week| {
            (0..7)
                .map(|day| start + Duration::days((week * 7 + day) as i64))
                .collect::<Vec<_>>()
        })
        .collect()
}

pub fn weekday_headers() -> [Weekday; 7] {
    [
        Weekday::Sun,
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
    ]
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let next_month = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).expect("valid next month")
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).expect("valid next month")
    };
    (next_month - Duration::days(1)).day()
}

#[cfg(test)]
mod tests {
    use super::{month_grid, month_start, shift_date_by_months, shift_month};
    use chrono::NaiveDate;

    #[test]
    fn month_grid_starts_on_sunday_boundary() {
        let march = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let grid = month_grid(march);
        assert_eq!(grid.len(), 6);
        assert_eq!(grid[0].len(), 7);
        assert_eq!(
            grid[0][0],
            NaiveDate::from_ymd_opt(2026, 3, 1).expect("date")
        );
        assert_eq!(
            grid[5][6],
            NaiveDate::from_ymd_opt(2026, 4, 11).expect("date")
        );
    }

    #[test]
    fn month_grid_includes_leading_days_for_midweek_month() {
        let april = NaiveDate::from_ymd_opt(2026, 4, 20).expect("date");
        let grid = month_grid(april);
        assert_eq!(
            grid[0][0],
            NaiveDate::from_ymd_opt(2026, 3, 29).expect("date")
        );
        assert_eq!(
            grid[0][3],
            NaiveDate::from_ymd_opt(2026, 4, 1).expect("date")
        );
    }

    #[test]
    fn shift_month_moves_between_year_boundaries() {
        let january = NaiveDate::from_ymd_opt(2026, 1, 15).expect("date");
        assert_eq!(
            shift_month(january, -1),
            NaiveDate::from_ymd_opt(2025, 12, 1).expect("date")
        );
        assert_eq!(
            shift_month(january, 14),
            NaiveDate::from_ymd_opt(2027, 3, 1).expect("date")
        );
    }

    #[test]
    fn shift_date_by_months_clamps_to_valid_day() {
        let january_31 = NaiveDate::from_ymd_opt(2026, 1, 31).expect("date");
        assert_eq!(
            shift_date_by_months(january_31, 1),
            NaiveDate::from_ymd_opt(2026, 2, 28).expect("date")
        );
    }

    #[test]
    fn month_start_normalizes_day_to_first() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 19).expect("date");
        assert_eq!(
            month_start(date),
            NaiveDate::from_ymd_opt(2026, 8, 1).expect("date")
        );
    }
}
