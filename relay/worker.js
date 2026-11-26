// Burin's report relay: takes a report Burin posts (JSON: kind, title, text, version, language)
// and passes it on by mail to the author, through Cloudflare Email Routing. The author's address
// is a secret of the Worker (REPORT_TO), never in this file. Sends only to that one address.
import { EmailMessage } from "cloudflare:email";

const LONGEST_BODY = 40000; // bytes of JSON taken in
const LONGEST_TITLE = 200;
const LONGEST_TEXT = 30000;
const PER_HOUR = 5; // reports from one address in an hour (per running copy of the relay)
const KINDS = ["Problem", "Idea", "Question", "Crash"];

const recent = new Map();

function answer(status, body) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" },
  });
}

// Text as base64 of its UTF-8, in lines of 76 for a mail body.
function base64(text) {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}

function tooMany(who) {
  const now = Date.now();
  const times = (recent.get(who) || []).filter((t) => now - t < 3600_000);
  if (times.length >= PER_HOUR) {
    recent.set(who, times);
    return true;
  }
  times.push(now);
  recent.set(who, times);
  if (recent.size > 10000) recent.clear();
  return false;
}

export default {
  async fetch(request, env) {
    if (request.method === "OPTIONS") {
      return new Response(null, {
        status: 204,
        headers: { "Access-Control-Allow-Origin": "*", "Access-Control-Allow-Methods": "POST", "Access-Control-Allow-Headers": "Content-Type" },
      });
    }
    if (request.method !== "POST") return answer(405, { ok: false, error: "POST a report" });
    const raw = await request.text();
    if (raw.length > LONGEST_BODY) return answer(413, { ok: false, error: "too long" });
    let report;
    try {
      report = JSON.parse(raw);
    } catch {
      return answer(400, { ok: false, error: "not JSON" });
    }
    const kind = KINDS.includes(report.kind) ? report.kind : "Problem";
    const title = String(report.title || "").replace(/[\r\n]+/g, " ").trim().slice(0, LONGEST_TITLE);
    const text = String(report.text || "").slice(0, LONGEST_TEXT);
    const version = String(report.version || "?").replace(/[^0-9A-Za-z.+-]/g, "").slice(0, 32);
    const language = report.language === "fr" ? "fr" : "en";
    if (!title) return answer(400, { ok: false, error: "no title" });
    const who = request.headers.get("CF-Connecting-IP") || "unknown";
    if (tooMany(who)) return answer(429, { ok: false, error: "too many reports" });
    if (!env.REPORT_TO || !env.REPORT_FROM || !env.MAIL) return answer(503, { ok: false, error: "relay not set up" });

    const domain = env.REPORT_FROM.split("@")[1] || "localhost";
    const id = `<${crypto.randomUUID()}@${domain}>`;
    const subject = `Burin ${version}: [${kind}] ${title}`;
    const body = `${text}\n\n--\nBurin ${version}, ${language}\n`;
    const mime = [
      `From: Burin <${env.REPORT_FROM}>`,
      `To: <${env.REPORT_TO}>`,
      `Subject: =?UTF-8?B?${base64(subject)}?=`,
      `Message-ID: ${id}`,
      `Date: ${new Date().toUTCString()}`,
      "MIME-Version: 1.0",
      "Content-Type: text/plain; charset=utf-8",
      "Content-Transfer-Encoding: base64",
      "",
      base64(body).replace(/(.{76})/g, "$1\r\n"),
      "",
    ].join("\r\n");
    try {
      await env.MAIL.send(new EmailMessage(env.REPORT_FROM, env.REPORT_TO, mime));
    } catch (e) {
      return answer(502, { ok: false, error: "could not pass it on" });
    }
    return answer(200, { ok: true });
  },
};
