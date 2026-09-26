import type { LayoutProps, Metadata } from "@farm.js/core";
import { defineLayoutFonts, localFont } from "@farm.js/core/font";
import "./globals.css";

const geistSans = localFont({
  src: "geist/dist/fonts/geist-sans/Geist-Variable.woff2",
  family: "Geist Sans",
  weight: "100 900",
  variable: "--font-geist-sans",
  fallback: ["system-ui", "sans-serif"],
});

const geistMono = localFont({
  src: "geist/dist/fonts/geist-mono/GeistMono-Variable.woff2",
  family: "Geist Mono",
  weight: "100 900",
  variable: "--font-geist-mono",
  fallback: ["ui-monospace", "monospace"],
});

export const fonts = defineLayoutFonts({
  body: geistSans,
  code: geistMono,
});

export const metadata: Metadata = {
  title: "Jevia — Outcome-aware model routing",
  description:
    "Route coding tasks to the right model, verify the result, and use real outcomes to improve the next decision.",
  icons: {
    icon: [{ url: "/favicon.svg", type: "image/svg+xml", sizes: "any" }],
  },
};

export default function RootLayout({ children }: LayoutProps) {
  return (
    <html lang="en" className={`${geistSans.variable} ${geistMono.variable}`}>
      <head>
        <meta charSet="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <meta name="theme-color" content="#0a0a0a" />
      </head>
      <body className={geistSans.className}>{children}</body>
    </html>
  );
}
