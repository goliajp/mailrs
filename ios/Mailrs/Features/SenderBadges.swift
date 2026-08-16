import SwiftUI

/// The three marks a sender can wear, and what each of them claims.
///
/// Split out of `ThreadView.swift` at the 500-line limit. `private` came
/// off on the way: in Swift that is file scope, and these are drawn from
/// a file that is no longer this one.

/// What the server's cryptographic checks concluded about the sender.
///
/// **Warnings only. There is no positive mark, and that is deliberate.**
///
/// There was one: `verified` drew a green seal on a DMARC pass. A JCB
/// phishing mail earned it on 2026-08-16 — `spf=pass dkim=pass
/// dmarc=pass`, all three correct, because the attacker owns
/// `wokjx.crabfishhh.com` and authentication records are free on a
/// domain you control. DMARC's claim is that the mail came from the
/// domain in the From header; the lie was the display name, which DMARC
/// does not authenticate. The seal sat beside the words that were the
/// deception.
///
/// Gmail's check mark requires DMARC at an enforced policy, BIMI, and a
/// Verified Mark Certificate — a CA verifying trademark ownership. It
/// stands for an identity checked out of band, never for a passing
/// authentication run. We have no VMC, so we show no mark.
///
/// The asymmetry is the design: a warning that is sometimes wrong costs
/// a reader distrusting real mail; a mark that is sometimes wrong costs
/// a reader trusting a fake one, and the attacker chose it.
struct SenderTrustBadge: View {
    let verdict: String

    var body: some View {
        switch verdict {
        case "suspicious":
            // A mark, not a sentence. "Unverified sender" spelled out
            // beside a name and a date is two words too many for the
            // line, and the header wrapped — which reads as a defect
            // whatever it says. Colour and shape carry it; the words
            // stay as the accessibility label, where they are read
            // aloud rather than competing for width.
            Image(systemName: "exclamationmark.shield.fill")
                .font(.footnote)
                .foregroundStyle(.orange)
                .accessibilityLabel("Unverified sender")
        default:
            EmptyView()
        }
    }
}


/// Which of my addresses this arrived at, when it was not the obvious
/// one.
///
/// Mail to `sales@` and mail to `lihao@` land in the same mailbox and
/// looked identical once they got there. The address a message was sent
/// to is part of what it is: it decides whether to answer as a person
/// or as a desk, and an address only one service was ever given makes
/// mail arriving at it suspect on its own.
///
/// Same symbol as the Aliases screen, so the mark and the place you
/// manage it are recognisably the same subject.
struct AliasBadge: View {
    let alias: String?

    var body: some View {
        if let alias {
            HStack(spacing: 3) {
                Image(systemName: "arrow.triangle.branch")
                    .font(.system(size: 9, weight: .semibold))
                Text(verbatim: alias)
                    .font(.caption2)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            .foregroundStyle(Color.accentColor)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(Color.accentColor.opacity(0.12), in: Capsule())
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Delivered to \(alias)")
        }
    }
}


/// Where the message actually came from, when its sender's name says
/// somewhere else.
///
/// The thread header shows a display name and never an address, which
/// is precisely the gap brand impersonation lives in: `Amazon.co.jp`
/// reads as Amazon whether it was sent by Amazon or by
/// `mail07.jqjintaiyang.com`. Measured on this mailbox — 1,500 From
/// headers, 206 names containing a domain, **8** disagreeing with the
/// domain that sent them, six of those unmistakable — so it is rare
/// enough to be worth interrupting for.
///
/// It states rather than accuses. "This came from X" is useful even
/// when X turns out to be the same company's second domain, which is
/// what makes it safe to show on a signal that cannot be perfect.
struct SenderClaimBadge: View {
    let actualDomain: String?

    var body: some View {
        if let actualDomain {
            HStack(spacing: 3) {
                Image(systemName: "questionmark.circle.fill")
                    .font(.system(size: 9, weight: .semibold))
                Text(verbatim: actualDomain)
                    .font(.caption2)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            .foregroundStyle(.orange)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(Color.orange.opacity(0.14), in: Capsule())
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Sent from \(actualDomain)")
        }
    }
}
