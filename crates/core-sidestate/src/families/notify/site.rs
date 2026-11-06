//! Every account's push counters summed per channel per day, for the
//! administrators' overview.

use serde::Serialize;

use super::{CHANNELS, day_of, stats_key};

/// One channel's pushes on one day, summed over accounts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ChannelTotals {
    pub sent: i64,
    pub failed: i64,
    /// held back by the reader's push choices
    pub suppressed_policy: i64,
    /// held back by the reader's quiet hours
    pub suppressed_quiet: i64,
}

/// One day of pushes across the site, per channel.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SiteDay {
    pub day: String,
    pub apns: ChannelTotals,
    pub web: ChannelTotals,
    pub unifiedpush: ChannelTotals,
}

/// The outcomes summed, in [`ChannelTotals`] field order; clicks are left out.
const SUMMED: [&str; 4] = ["sent", "failed", "suppressed_policy", "suppressed_quiet"];

/// The last `days` days of every one of `users`' counters, summed per
/// channel and outcome, newest first. One MGET per user.
pub fn site_stats(
    conn: &mut kevy_client::Connection,
    users: &[String],
    days: u32,
    now: i64,
) -> std::io::Result<Vec<SiteDay>> {
    let day_names: Vec<String> = (0..i64::from(days))
        .map(|back| day_of(now - back * 86_400))
        .collect();
    // [day][channel][outcome], in CHANNELS and SUMMED order, the order the keys are built in
    let per_day = CHANNELS.len() * SUMMED.len();
    let mut sums = vec![[[0i64; SUMMED.len()]; CHANNELS.len()]; day_names.len()];
    for user in users {
        let keys: Vec<String> = day_names
            .iter()
            .flat_map(|day| {
                CHANNELS.iter().flat_map(move |channel| {
                    SUMMED
                        .iter()
                        .map(move |outcome| stats_key(user, day, channel, outcome))
                })
            })
            .collect();
        let refs: Vec<&[u8]> = keys.iter().map(|k| k.as_bytes()).collect();
        for (i, v) in conn.mget(&refs)?.into_iter().enumerate() {
            let n: i64 = v
                .and_then(|v| String::from_utf8_lossy(&v).parse().ok())
                .unwrap_or(0);
            sums[i / per_day][(i / SUMMED.len()) % CHANNELS.len()][i % SUMMED.len()] += n;
        }
    }
    let totals = |c: [i64; 4]| ChannelTotals {
        sent: c[0],
        failed: c[1],
        suppressed_policy: c[2],
        suppressed_quiet: c[3],
    };
    Ok(day_names
        .into_iter()
        .zip(sums)
        .map(|(day, [apns, web, unifiedpush])| SiteDay {
            day,
            apns: totals(apns),
            web: totals(web),
            unifiedpush: totals(unifiedpush),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::count;
    use super::*;

    #[test]
    fn site_stats_sum_the_listed_accounts_per_channel_and_day() {
        let mut c = kevy_client::Connection::connect("mem://notify-site-stats").unwrap();
        let now = 1_793_500_000;
        let yesterday = now - 86_400;
        count(&mut c, "a@x", "apns", "sent", now).unwrap();
        count(&mut c, "a@x", "apns", "sent", now).unwrap();
        count(&mut c, "b@x", "apns", "sent", now).unwrap();
        count(&mut c, "b@x", "web", "failed", now).unwrap();
        count(&mut c, "a@x", "web", "clicked", now).unwrap();
        count(&mut c, "a@x", "unifiedpush", "suppressed_quiet", yesterday).unwrap();
        count(&mut c, "b@x", "unifiedpush", "suppressed_policy", yesterday).unwrap();
        // not an account any more: not in the sum
        count(&mut c, "gone@x", "apns", "sent", now).unwrap();

        let users = vec!["a@x".to_string(), "B@x".to_string()];
        let s = site_stats(&mut c, &users, 3, now).unwrap();
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].day, day_of(now));
        assert_eq!(s[0].apns.sent, 3);
        assert_eq!(s[0].web.failed, 1);
        assert_eq!(s[0].web.sent, 0, "a click is not a send");
        assert_eq!(s[1].day, day_of(yesterday));
        assert_eq!(s[1].unifiedpush.suppressed_quiet, 1);
        assert_eq!(s[1].unifiedpush.suppressed_policy, 1);
        assert_eq!(
            s[2],
            SiteDay {
                day: day_of(now - 2 * 86_400),
                ..Default::default()
            }
        );
        assert!(site_stats(&mut c, &[], 2, now).unwrap()[0].apns == ChannelTotals::default());
    }
}
