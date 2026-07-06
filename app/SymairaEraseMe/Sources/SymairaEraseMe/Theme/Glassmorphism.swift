import SwiftUI

/// Glassmorphism card style matching the HTML dashboard aesthetic.
struct GlassCardModifier: ViewModifier {
    var cornerRadius: CGFloat = 12
    var padding: CGFloat = 16

    func body(content: Content) -> some View {
        content
            .padding(padding)
            .background(
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .fill(BrandColors.bgCard)
                    .overlay(
                        RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                            .stroke(Color.white.opacity(0.05), lineWidth: 1)
                    )
            )
    }
}

extension View {
    func glassCard(cornerRadius: CGFloat = 12, padding: CGFloat = 16) -> some View {
        modifier(GlassCardModifier(cornerRadius: cornerRadius, padding: padding))
    }
}

/// Status badge pill used throughout the dashboard.
struct StatusBadge: View {
    let status: String

    var body: some View {
        Text(displayText)
            .font(.caption)
            .fontWeight(.medium)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(
                Capsule()
                    .fill(BrandColors.backgroundColor(for: status))
            )
            .foregroundStyle(BrandColors.color(for: status))
    }

    private var displayText: String {
        switch status.uppercased() {
        case "PLANNED": return "Planned"
        case "SENT": return "Sent"
        case "AWAITING_ACK": return "Awaiting ACK"
        case "AWAITING_RESPONSE": return "Awaiting Response"
        case "CONFIRMED": return "Confirmed"
        case "REJECTED", "REJECTED_FINAL": return "Rejected"
        case "OVERDUE": return "Overdue"
        default: return status
        }
    }
}

/// Summary stat card for the dashboard.
struct StatCard: View {
    let title: String
    let value: Int
    let color: Color
    var subtitle: String? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title)
                .font(.caption)
                .foregroundStyle(BrandColors.textSecondary)
                .textCase(.uppercase)
                .kerning(0.8)
            Text("\(value)")
                .font(.system(size: 34, weight: .bold, design: .rounded))
                .foregroundStyle(color)
            if let subtitle {
                Text(subtitle)
                    .font(.caption2)
                    .foregroundStyle(BrandColors.textMuted)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .overlay(
            ZStack {
                RadialGradient(
                    colors: [color.opacity(0.08), Color.clear],
                    center: .topTrailing,
                    startRadius: 0,
                    endRadius: 80
                )
            }
            .blendMode(.screen)
        )
        .interactiveGlassCard()
    }
}


/// Placeholder shown when a view has no data.
struct EmptyStateView: View {
    let icon: String
    let title: String
    let message: String

    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: icon)
                .font(.system(size: 40))
                .foregroundStyle(BrandColors.textMuted)
            Text(title)
                .font(.headline)
                .foregroundStyle(BrandColors.textSecondary)
            Text(message)
                .font(.subheadline)
                .foregroundStyle(BrandColors.textMuted)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding()
    }
}

/// Error banner for displaying tool call failures.
struct ErrorBanner: View {
    let message: String
    var onDismiss: (() -> Void)? = nil

    var body: some View {
        HStack {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(BrandColors.rejected)
            Text(message)
                .font(.subheadline)
                .foregroundStyle(BrandColors.textPrimary)
            Spacer()
            if let onDismiss {
                Button(action: onDismiss) {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(BrandColors.textMuted)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(10)
        .background(
            RoundedRectangle(cornerRadius: 8)
                .fill(BrandColors.rejectedBg)
                .overlay(
                    RoundedRectangle(cornerRadius: 8)
                        .stroke(BrandColors.rejectedBorder, lineWidth: 1)
                )
        )
    }
}

/// Loading spinner overlay.
struct LoadingOverlay: View {
    let message: String

    var body: some View {
        VStack(spacing: 12) {
            ProgressView()
                .tint(BrandColors.goldPrimary)
            Text(message)
                .font(.subheadline)
                .foregroundStyle(BrandColors.textSecondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// A beautiful tiled blueprint grid background with ambient glowing radial blobs
/// matching the Champagne Gold and Warm Sand theme.
struct BlueprintBackground: View {
    var body: some View {
        ZStack {
            BrandColors.bgDark.ignoresSafeArea()
            
            // Subtle dot grid
            Canvas { context, size in
                let spacing: CGFloat = 24
                var x: CGFloat = 0
                while x < size.width {
                    var y: CGFloat = 0
                    while y < size.height {
                        let rect = CGRect(x: x, y: y, width: 1.5, height: 1.5)
                        context.fill(Path(ellipseIn: rect), with: .color(BrandColors.goldPrimary.opacity(0.045)))
                        y += spacing
                    }
                    x += spacing
                }
            }
            .ignoresSafeArea()
            
            // Top Right ambient glow
            RadialGradient(
                colors: [BrandColors.goldPrimary.opacity(0.06), Color.clear],
                center: .topTrailing,
                startRadius: 0,
                endRadius: 550
            )
            .ignoresSafeArea()
            
            // Bottom Left ambient glow
            RadialGradient(
                colors: [BrandColors.goldShadow.opacity(0.04), Color.clear],
                center: .bottomLeading,
                startRadius: 0,
                endRadius: 550
            )
            .ignoresSafeArea()
        }
    }
}

/// Interactive Glassmorphism card style that scales and highlights its border on hover.
struct InteractiveGlassCardModifier: ViewModifier {
    var cornerRadius: CGFloat = 12
    var padding: CGFloat = 16
    @State private var isHovered = false

    func body(content: Content) -> some View {
        content
            .padding(padding)
            .background(
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .fill(isHovered ? BrandColors.bgCardHover : BrandColors.bgCard)
                    .overlay(
                        RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                            .stroke(isHovered ? BrandColors.goldPrimary.opacity(0.2) : Color.white.opacity(0.05), lineWidth: 1)
                    )
            )
            .scaleEffect(isHovered ? 1.008 : 1.0)
            .shadow(color: isHovered ? BrandColors.goldShadow.opacity(0.08) : Color.clear, radius: 10, x: 0, y: 5)
            .onHover { hovering in
                withAnimation(.spring(response: 0.25, dampingFraction: 0.8)) {
                    isHovered = hovering
                }
            }
    }
}

extension View {
    func interactiveGlassCard(cornerRadius: CGFloat = 12, padding: CGFloat = 16) -> some View {
        modifier(InteractiveGlassCardModifier(cornerRadius: cornerRadius, padding: padding))
    }
}

