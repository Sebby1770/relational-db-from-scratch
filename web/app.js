const db = new MiniDB();
const sqlBox = document.getElementById("sql");
const result = document.getElementById("result");
const status = document.getElementById("status");
const schema = document.getElementById("schema");

const EXAMPLES = [
  ["Join orders", "SELECT users.name, orders.total\nFROM users\nJOIN orders ON users.id = orders.user_id\nWHERE orders.total > 100\nORDER BY 2 DESC;"],
  ["GROUP BY", "SELECT user_id, COUNT(*), SUM(total)\nFROM orders\nGROUP BY user_id\nORDER BY 1;"],
  ["LEFT JOIN", "SELECT users.name, orders.total\nFROM users\nLEFT JOIN orders ON users.id = orders.user_id\nORDER BY users.name;"],
  ["UNION / EXCEPT", "SELECT name FROM users WHERE active = true\nUNION\nSELECT name FROM users WHERE active = false\nORDER BY 1;"],
  ["COALESCE + CASE", "SELECT name, COALESCE(email, 'missing'), CASE WHEN active = true THEN 'yes' ELSE 'no' END\nFROM users\nORDER BY 1;"],
  ["CTAS", "CREATE TABLE actives AS SELECT id, name FROM users WHERE active = true;\nSELECT * FROM actives;"],
];

function refreshSchema() {
  schema.textContent = db.catalog();
}

function render(out) {
  if (out.kind === "ok") {
    result.innerHTML = `<p>${escapeHtml(out.message)}</p>`;
    return;
  }
  if (out.kind === "plan") {
    result.innerHTML = `<pre>${escapeHtml(out.plan)}</pre>`;
    return;
  }
  const head = out.columns.map((c) => `<th>${escapeHtml(c)}</th>`).join("");
  const body = out.rows.map((row) =>
    `<tr>${row.map((v) => `<td${v === null ? ' class="null"' : ""}>${v === null ? "NULL" : escapeHtml(v)}</td>`).join("")}</tr>`
  ).join("");
  result.innerHTML = `<table><thead><tr>${head}</tr></thead><tbody>${body}</tbody></table>`;
}

function escapeHtml(v) {
  return String(v).replace(/[&<>"']/g, (ch) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[ch]));
}

function runAll(text, explain) {
  const chunks = text.split(";").map((s) => s.trim()).filter(Boolean);
  let last = { kind: "ok", message: "ok" };
  chunks.forEach((chunk) => {
    last = db.execute(explain ? `EXPLAIN ${chunk}` : chunk);
  });
  return last;
}

function run(explain = false) {
  status.classList.remove("err");
  try {
    const out = runAll(sqlBox.value, explain);
    status.textContent = out.kind === "rows" ? `${out.rows.length} row(s)` : out.kind === "plan" ? "plan" : out.message;
    render(out);
    refreshSchema();
  } catch (err) {
    status.classList.add("err");
    status.textContent = err.message;
    result.innerHTML = "";
  }
}

document.getElementById("btn-run").addEventListener("click", () => run(false));
document.getElementById("btn-explain").addEventListener("click", () => run(true));
document.getElementById("btn-reset").addEventListener("click", () => {
  seedDemo(db);
  refreshSchema();
  status.textContent = "Demo tables reloaded.";
});
sqlBox.addEventListener("keydown", (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key === "Enter") run(false);
});

const box = document.getElementById("examples");
EXAMPLES.forEach(([label, sql]) => {
  const btn = document.createElement("button");
  btn.textContent = label;
  btn.addEventListener("click", () => { sqlBox.value = sql; run(false); });
  box.appendChild(btn);
});

seedDemo(db);
refreshSchema();
run(false);
