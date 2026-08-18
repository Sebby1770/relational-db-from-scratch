class DbError extends Error {
  constructor(message) { super(message); this.name = "DbError"; }
}

class MiniDB {
  constructor() { this.tables = new Map(); }

  reset() { this.tables = new Map(); }

  execute(sql) {
    const text = String(sql || "").trim().replace(/;$/, "");
    if (!text) return { kind: "ok", message: "empty" };
    const upper = text.replace(/\s+/g, " ").toUpperCase();
    if (upper.startsWith("EXPLAIN ")) {
      const inner = this.execute(text.slice(8));
      return { kind: "plan", plan: this.explain(inner, text.slice(8)) };
    }
    if (upper.startsWith("CREATE TABLE")) return this.createTable(text);
    if (upper.startsWith("DROP TABLE")) return this.dropTable(text);
    if (upper.startsWith("TRUNCATE")) return this.truncate(text);
    if (upper.startsWith("ALTER TABLE") && /RENAME TO/i.test(text)) return this.rename(text);
    if (upper.startsWith("ALTER TABLE") && /ADD COLUMN/i.test(text)) return this.addColumn(text);
    if (upper.startsWith("INSERT INTO")) return this.insert(text);
    if (upper.startsWith("DELETE FROM")) return this.delete(text);
    if (upper.startsWith("UPDATE ")) return this.update(text);
    if (upper.startsWith("SELECT")) return this.select(text);
    throw new DbError("unsupported statement");
  }

  createTable(sql) {
    const as = sql.match(/^CREATE TABLE(?: IF NOT EXISTS)?\s+(\w+)\s+AS\s+(SELECT[\s\S]+)$/i);
    if (as) {
      const name = as[1].toLowerCase();
      if (this.tables.has(name) && /IF NOT EXISTS/i.test(sql)) return { kind: "ok", message: `created table ${name}` };
      const selected = this.select(as[2]);
      const columns = selected.columns.map((col, i) => {
        const sample = selected.rows.find((row) => row[i] !== null);
        const type = typeof sample === "number" ? "INT" : typeof sample === "boolean" ? "BOOL" : "TEXT";
        return { name: col.split(".").pop(), type };
      });
      this.tables.set(name, { name, columns, rows: selected.rows.map((row) => row.slice()) });
      return { kind: "ok", message: `created table ${name}` };
    }
    const m = sql.match(/^CREATE TABLE(?: IF NOT EXISTS)?\s+(\w+)\s*\((.+)\)\s*$/i);
    if (!m) throw new DbError("bad CREATE TABLE");
    const name = m[1].toLowerCase();
    if (this.tables.has(name) && /IF NOT EXISTS/i.test(sql)) return { kind: "ok", message: `created table ${name}` };
    if (this.tables.has(name)) throw new DbError(`table already exists: ${name}`);
    const columns = splitArgs(m[2]).map((part) => {
      const bits = part.trim().split(/\s+/);
      return { name: bits[0].toLowerCase(), type: (bits[1] || "TEXT").toUpperCase() };
    });
    this.tables.set(name, { name, columns, rows: [] });
    return { kind: "ok", message: `created table ${name}` };
  }

  dropTable(sql) {
    const m = sql.match(/^DROP TABLE(?: IF EXISTS)?\s+(\w+)$/i);
    if (!m) throw new DbError("bad DROP TABLE");
    const name = m[1].toLowerCase();
    if (!this.tables.has(name) && /IF EXISTS/i.test(sql)) return { kind: "ok", message: `dropped table ${name}` };
    if (!this.tables.delete(name)) throw new DbError(`table not found: ${name}`);
    return { kind: "ok", message: `dropped table ${name}` };
  }

  truncate(sql) {
    const m = sql.match(/^TRUNCATE(?: TABLE)?\s+(\w+)$/i);
    if (!m) throw new DbError("bad TRUNCATE");
    const table = this.table(m[1]);
    const count = table.rows.length;
    table.rows = [];
    return { kind: "ok", message: `deleted ${count} row(s)` };
  }

  rename(sql) {
    const m = sql.match(/^ALTER TABLE\s+(\w+)\s+RENAME TO\s+(\w+)$/i);
    if (!m) throw new DbError("bad ALTER RENAME");
    const from = m[1].toLowerCase();
    const to = m[2].toLowerCase();
    const table = this.table(from);
    if (this.tables.has(to)) throw new DbError(`table already exists: ${to}`);
    this.tables.delete(from);
    table.name = to;
    this.tables.set(to, table);
    return { kind: "ok", message: `renamed table ${from} to ${to}` };
  }

  addColumn(sql) {
    const m = sql.match(/^ALTER TABLE\s+(\w+)\s+ADD COLUMN\s+(\w+)\s+(\w+)$/i);
    if (!m) throw new DbError("bad ALTER ADD COLUMN");
    const table = this.table(m[1]);
    const name = m[2].toLowerCase();
    if (table.columns.some((c) => c.name === name)) throw new DbError(`column already exists: ${name}`);
    table.columns.push({ name, type: m[3].toUpperCase() });
    table.rows.forEach((row) => row.push(null));
    return { kind: "ok", message: `added column ${name} to ${table.name}` };
  }

  insert(sql) {
    const sel = sql.match(/^INSERT INTO\s+(\w+)\s+(SELECT[\s\S]+)$/i);
    if (sel) {
      const table = this.table(sel[1]);
      const selected = this.select(sel[2]);
      selected.rows.forEach((row) => table.rows.push(this.coerce(table, row)));
      return { kind: "ok", message: `inserted ${selected.rows.length} row(s)` };
    }
    const m = sql.match(/^INSERT INTO\s+(\w+)\s+VALUES\s+(.+)$/i);
    if (!m) throw new DbError("bad INSERT");
    const table = this.table(m[1]);
    const tuples = m[2].split(/\)\s*,\s*\(/).map((chunk, i, arr) => {
      let s = chunk.trim();
      if (i === 0) s = s.replace(/^\(/, "");
      if (i === arr.length - 1) s = s.replace(/\)$/, "");
      return splitArgs(s).map(parseValue);
    });
    tuples.forEach((row) => table.rows.push(this.coerce(table, row)));
    return { kind: "ok", message: `inserted ${tuples.length} row(s)` };
  }

  delete(sql) {
    const m = sql.match(/^DELETE FROM\s+(\w+)(?:\s+WHERE\s+(.+))?$/i);
    if (!m) throw new DbError("bad DELETE");
    const table = this.table(m[1]);
    const before = table.rows.length;
    if (!m[2]) table.rows = [];
    else table.rows = table.rows.filter((row) => !this.matches(table, row, m[2]));
    return { kind: "ok", message: `deleted ${before - table.rows.length} row(s)` };
  }

  update(sql) {
    const m = sql.match(/^UPDATE\s+(\w+)\s+SET\s+(\w+)\s*=\s*(.+?)(?:\s+WHERE\s+(.+))?$/i);
    if (!m) throw new DbError("bad UPDATE (single assignment only in this playground)");
    const table = this.table(m[1]);
    const idx = this.colIndex(table, m[2]);
    const value = parseValue(m[3]);
    let count = 0;
    table.rows.forEach((row) => {
      if (!m[4] || this.matches(table, row, m[4])) { row[idx] = value; count += 1; }
    });
    return { kind: "ok", message: `updated ${count} row(s)` };
  }

  select(sql) {
    const parts = splitSetOps(sql);
    let acc = this.selectCore(parts[0].sql);
    for (let i = 1; i < parts.length; i += 1) {
      const next = this.selectCore(parts[i].sql);
      if (next.columns.length !== acc.columns.length) throw new DbError("set-op column count mismatch");
      if (parts[i].op === "UNION ALL") acc.rows = acc.rows.concat(next.rows);
      else if (parts[i].op === "UNION") acc.rows = uniqueRows(acc.rows.concat(next.rows));
      else if (parts[i].op === "EXCEPT") acc.rows = exceptRows(acc.rows, next.rows);
      else if (parts[i].op === "INTERSECT") acc.rows = intersectRows(acc.rows, next.rows);
    }
    return acc;
  }

  selectCore(sql) {
    const parsed = parseSelect(sql);
    const left = this.table(parsed.table);
    const schema = left.columns.map((c) => ({ ...c, qual: `${left.name}.${c.name}` }));
    let rows = left.rows.map((row) => row.slice());

    parsed.joins.forEach((join) => {
      const right = this.table(join.table);
      const rSchema = right.columns.map((c) => ({ ...c, qual: `${right.name}.${c.name}` }));
      if (join.type === "CROSS") {
        const next = [];
        rows.forEach((l) => right.rows.forEach((r) => next.push(l.concat(r))));
        rows = next;
      } else {
        const li = resolveName(schema, join.left);
        const ri = resolveName(rSchema, join.right);
        const next = [];
        if (join.type === "RIGHT") {
          right.rows.forEach((r) => {
            let hit = false;
            rows.forEach((l) => {
              if (eq(l[li], r[ri])) { next.push(l.concat(r)); hit = true; }
            });
            if (!hit) next.push(Array(schema.length).fill(null).concat(r));
          });
        } else {
          const buckets = new Map();
          right.rows.forEach((r) => {
            if (r[ri] === null) return;
            const key = JSON.stringify(r[ri]);
            if (!buckets.has(key)) buckets.set(key, []);
            buckets.get(key).push(r);
          });
          rows.forEach((l) => {
            const matches = l[li] === null ? [] : (buckets.get(JSON.stringify(l[li])) || []);
            if (matches.length) matches.forEach((r) => next.push(l.concat(r)));
            else if (join.type === "LEFT") next.push(l.concat(Array(rSchema.length).fill(null)));
          });
        }
        rows = next;
      }
      schema.push(...rSchema);
    });

    if (parsed.where) rows = rows.filter((row) => this.matchesSchema(schema, row, parsed.where));

    const grouped = parsed.group || parsed.items.some((item) => item.agg);
    let outCols;
    let outRows;
    if (grouped) {
      const gidx = (parsed.group || []).map((name) => resolveName(schema, name));
      const groups = new Map();
      rows.forEach((row) => {
        const key = JSON.stringify(gidx.map((i) => row[i]));
        if (!groups.has(key)) groups.set(key, []);
        groups.get(key).push(row);
      });
      outCols = parsed.items.map(itemOutput);
      outRows = [...groups.values()].map((group) => parsed.items.map((item) => evalItem(schema, group, item)));
    } else {
      outCols = parsed.items.map(itemOutput);
      outRows = rows.map((row) => parsed.items.map((item) => evalItem(schema, [row], item)));
    }

    if (parsed.distinct) outRows = uniqueRows(outRows);
    if (parsed.order.length) {
      outRows.sort((a, b) => {
        for (const key of parsed.order) {
          const idx = /^\d+$/.test(key.col) ? Number(key.col) - 1 : outCols.findIndex((c) => c === key.col || c.endsWith(`.${key.col}`));
          const cmp = cmpVal(a[idx], b[idx]);
          if (cmp) return key.dir === "DESC" ? -cmp : cmp;
        }
        return 0;
      });
    }
    if (parsed.offset) outRows = outRows.slice(parsed.offset);
    if (parsed.limit != null) outRows = outRows.slice(0, parsed.limit);
    return { kind: "rows", columns: outCols, rows: outRows };
  }

  matches(table, row, pred) { return this.matchesSchema(table.columns.map((c) => ({ ...c, qual: `${table.name}.${c.name}` })), row, pred); }

  matchesSchema(schema, row, pred) {
    const parts = splitBool(pred, "OR");
    if (parts.length > 1) return parts.some((p) => this.matchesSchema(schema, row, p));
    const ands = splitBool(pred, "AND");
    if (ands.length > 1) return ands.every((p) => this.matchesSchema(schema, row, p));
    const isNull = pred.match(/^(\S+)\s+IS(\s+NOT)?\s+NULL$/i);
    if (isNull) {
      const v = row[resolveName(schema, isNull[1])];
      return isNull[2] ? v !== null : v === null;
    }
    const like = pred.match(/^(\S+)\s+LIKE\s+('.+')$/i);
    if (like) {
      const v = String(row[resolveName(schema, like[1])] ?? "");
      const pattern = parseValue(like[2]);
      const re = new RegExp("^" + String(pattern).replace(/%/g, ".*") + "$", "i");
      return re.test(v);
    }
    const between = pred.match(/^(\S+)\s+(NOT\s+)?BETWEEN\s+(.+)\s+AND\s+(.+)$/i);
    if (between) {
      const v = row[resolveName(schema, between[1])];
      const ok = cmpVal(v, parseValue(between[3])) >= 0 && cmpVal(v, parseValue(between[4])) <= 0;
      return between[2] ? !ok : ok;
    }
    const inn = pred.match(/^(\S+)\s+(NOT\s+)?IN\s*\((.+)\)$/i);
    if (inn) {
      const v = row[resolveName(schema, inn[1])];
      const list = splitArgs(inn[3]).map(parseValue);
      const ok = list.some((x) => eq(x, v));
      return inn[2] ? !ok : ok;
    }
    const cmp = pred.match(/^(\S+)\s*(=|!=|<>|<=|>=|<|>)\s*(.+)$/);
    if (!cmp) throw new DbError(`bad predicate: ${pred}`);
    const left = row[resolveName(schema, cmp[1])];
    const right = parseValue(cmp[3]);
    if (left === null || right === null) return false;
    const c = cmpVal(left, right);
    return { "=": c === 0, "!=": c !== 0, "<>": c !== 0, "<": c < 0, ">": c > 0, "<=": c <= 0, ">=": c >= 0 }[cmp[2]];
  }

  coerce(table, row) {
    if (row.length !== table.columns.length) throw new DbError(`row has ${row.length} values but table expects ${table.columns.length}`);
    return row.map((value, i) => {
      if (value === null) return null;
      const type = table.columns[i].type;
      if (type.startsWith("INT") && typeof value !== "number") throw new DbError(`type mismatch for ${table.columns[i].name}`);
      if (type.startsWith("BOOL") && typeof value !== "boolean") throw new DbError(`type mismatch for ${table.columns[i].name}`);
      return value;
    });
  }

  table(name) {
    const table = this.tables.get(String(name).toLowerCase());
    if (!table) throw new DbError(`table not found: ${name}`);
    return table;
  }

  colIndex(table, name) {
    const idx = table.columns.findIndex((c) => c.name === name.toLowerCase());
    if (idx < 0) throw new DbError(`column not found: ${name}`);
    return idx;
  }

  catalog() {
    return [...this.tables.values()].map((t) =>
      `${t.name} (${t.rows.length})\n` + t.columns.map((c) => `  ${c.name} ${c.type}`).join("\n")
    ).join("\n\n") || "(no tables)";
  }

  explain(result, sql) {
    const lines = ["Select"];
    if (/join/i.test(sql)) lines.push(/left join/i.test(sql) ? "  Join: nested loop join LEFT" : /cross join/i.test(sql) ? "  Join: cross product CROSS" : "  Join: hash join INNER");
    if (/group by/i.test(sql)) lines.push("  Aggregate: hash group by");
    if (/union|except|intersect/i.test(sql)) lines.push("  SetOp");
    if (result.kind === "rows") lines.push(`  Rows: ${result.rows.length}`);
    return lines.join("\n");
  }
}

function parseSelect(sql) {
  const distinct = /^\s*SELECT\s+DISTINCT\s+/i.test(sql);
  const body = sql.replace(/^\s*SELECT\s+(DISTINCT\s+)?/i, "");
  const fromAt = body.search(/\sFROM\s/i);
  if (fromAt < 0) throw new DbError("SELECT requires FROM");
  const projection = body.slice(0, fromAt).trim();
  let rest = body.slice(fromAt + 5).trim();
  const tableMatch = rest.match(/^(\w+)/);
  const table = tableMatch[1];
  rest = rest.slice(table.length).trim();
  const joins = [];
  while (/^(INNER\s+JOIN|LEFT(?:\s+OUTER)?\s+JOIN|RIGHT(?:\s+OUTER)?\s+JOIN|CROSS\s+JOIN|JOIN)\b/i.test(rest)) {
    const jm = rest.match(/^(INNER\s+JOIN|LEFT(?:\s+OUTER)?\s+JOIN|RIGHT(?:\s+OUTER)?\s+JOIN|CROSS\s+JOIN|JOIN)\s+(\w+)(?:\s+ON\s+(\S+)\s*=\s+(\S+))?/i);
    if (!jm) break;
    const kind = jm[1].toUpperCase();
    joins.push({
      type: kind.startsWith("LEFT") ? "LEFT" : kind.startsWith("RIGHT") ? "RIGHT" : kind.startsWith("CROSS") ? "CROSS" : "INNER",
      table: jm[2],
      left: jm[3],
      right: jm[4],
    });
    rest = rest.slice(jm[0].length).trim();
  }
  let where, group, order = [], limit, offset;
  const take = (re) => {
    const m = rest.match(re);
    if (!m) return null;
    rest = rest.slice(m[0].length).trim();
    return m;
  };
  const w = take(/^WHERE\s+(.+?)(?=\s+GROUP\s+BY|\s+ORDER\s+BY|\s+LIMIT|\s+OFFSET|$)/i);
  if (w) where = w[1].trim();
  const g = take(/^GROUP\s+BY\s+(.+?)(?=\s+ORDER\s+BY|\s+LIMIT|\s+OFFSET|$)/i);
  if (g) group = splitArgs(g[1]);
  const o = take(/^ORDER\s+BY\s+(.+?)(?=\s+LIMIT|\s+OFFSET|$)/i);
  if (o) {
    order = splitArgs(o[1]).map((part) => {
      const bits = part.trim().split(/\s+/);
      return { col: bits[0], dir: (bits[1] || "ASC").toUpperCase() };
    });
  }
  const l = take(/^LIMIT\s+(\d+)/i);
  if (l) limit = Number(l[1]);
  const off = take(/^OFFSET\s+(\d+)/i);
  if (off) offset = Number(off[1]);
  const items = projection === "*"
    ? [{ star: true }]
    : splitArgs(projection).map(parseItem);
  return { distinct, table, joins, where, group, order, limit, offset, items };
}

function parseItem(raw) {
  const t = raw.trim();
  if (t === "*") return { star: true };
  const countStar = t.match(/^COUNT\(\*\)$/i);
  if (countStar) return { agg: "count", star: true, out: "count" };
  const agg = t.match(/^(COUNT|SUM|MIN|MAX|AVG)\((\w+(?:\.\w+)?)\)$/i);
  if (agg) return { agg: agg[1].toLowerCase(), col: agg[2], out: agg[1].toLowerCase() };
  const coal = t.match(/^COALESCE\((\w+(?:\.\w+)?)\s*,\s*(.+)\)$/i);
  if (coal) return { coalesce: true, col: coal[1], fallback: parseValue(coal[2]), out: "coalesce" };
  const cas = t.match(/^CASE\s+WHEN\s+(.+)\s+THEN\s+(.+)\s+ELSE\s+(.+)\s+END$/i);
  if (cas) return { case: true, when: cas[1], then: parseValue(cas[2]), else: parseValue(cas[3]), out: "case" };
  return { col: t, out: t.includes(".") ? t.toLowerCase() : t.toLowerCase() };
}

function itemOutput(item) { return item.out || item.col || "*"; }

function evalItem(schema, rows, item) {
  if (item.agg === "count" && item.star) return rows.length;
  if (item.agg === "count") return rows.filter((r) => r[resolveName(schema, item.col)] !== null).length;
  if (item.agg === "sum") return rows.reduce((s, r) => s + (Number(r[resolveName(schema, item.col)]) || 0), 0);
  if (item.agg === "min" || item.agg === "max") {
    const vals = rows.map((r) => r[resolveName(schema, item.col)]).filter((v) => v !== null);
    return vals.reduce((a, b) => (item.agg === "min" ? (cmpVal(a, b) < 0 ? a : b) : (cmpVal(a, b) > 0 ? a : b)));
  }
  if (item.agg === "avg") {
    const vals = rows.map((r) => r[resolveName(schema, item.col)]).filter((v) => typeof v === "number");
    return vals.length ? Math.trunc(vals.reduce((a, b) => a + b, 0) / vals.length) : null;
  }
  if (item.coalesce) {
    const v = rows[0][resolveName(schema, item.col)];
    return v === null ? item.fallback : v;
  }
  if (item.case) {
    const db = { matchesSchema: MiniDB.prototype.matchesSchema };
    return db.matchesSchema(schema, rows[0], item.when) ? item.then : item.else;
  }
  if (item.star) return rows[0];
  return rows[0][resolveName(schema, item.col)];
}

function resolveName(schema, name) {
  if (!name) throw new DbError("missing column");
  const n = name.toLowerCase();
  if (schema.length && schema[0].qual && n === "*") return 0;
  const hits = schema.map((c, i) => ({ c, i })).filter(({ c }) => c.name === n || c.qual === n);
  if (hits.length === 1) return hits[0].i;
  if (hits.length > 1) throw new DbError(`ambiguous column: ${name}`);
  throw new DbError(`column not found: ${name}`);
}

function parseValue(raw) {
  const t = String(raw).trim();
  if (/^null$/i.test(t)) return null;
  if (/^true$/i.test(t)) return true;
  if (/^false$/i.test(t)) return false;
  if (/^-?\d+$/.test(t)) return Number(t);
  if (/^'.*'$/.test(t)) return t.slice(1, -1).replace(/''/g, "'");
  throw new DbError(`expected literal, got ${t}`);
}

function splitArgs(text) {
  const out = []; let buf = ""; let depth = 0; let quote = false;
  for (const ch of text) {
    if (ch === "'" && !quote) quote = true;
    else if (ch === "'" && quote) quote = false;
    else if (!quote && ch === "(") depth += 1;
    else if (!quote && ch === ")") depth -= 1;
    if (ch === "," && !quote && depth === 0) { out.push(buf.trim()); buf = ""; continue; }
    buf += ch;
  }
  if (buf.trim()) out.push(buf.trim());
  return out;
}

function splitBool(text, word) {
  const re = new RegExp(`\\s${word}\\s`, "i");
  const out = []; let buf = ""; let depth = 0; let quote = false;
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i];
    if (ch === "'" && !quote) quote = true;
    else if (ch === "'" && quote) quote = false;
    else if (!quote && ch === "(") depth += 1;
    else if (!quote && ch === ")") depth -= 1;
    buf += ch;
    if (!quote && depth === 0 && re.test(buf.slice(-word.length - 2))) {
      out.push(buf.slice(0, -word.length - 2).trim());
      buf = "";
    }
  }
  if (buf.trim()) out.push(buf.trim());
  return out;
}

function splitSetOps(sql) {
  const parts = [];
  const re = /\s+(UNION ALL|UNION|EXCEPT|INTERSECT)\s+/ig;
  let last = 0; let m;
  const found = [];
  while ((m = re.exec(sql))) found.push(m);
  if (!found.length) return [{ op: null, sql }];
  found.forEach((hit, i) => {
    const chunk = sql.slice(last, hit.index);
    if (i === 0) parts.push({ op: null, sql: chunk.trim().replace(/^SELECT\s+/i, "SELECT ") });
    last = hit.index + hit[0].length;
    const nextEnd = found[i + 1] ? found[i + 1].index : sql.length;
    parts.push({ op: hit[1].toUpperCase(), sql: ("SELECT " + sql.slice(last, nextEnd)).replace(/^SELECT\s+SELECT/i, "SELECT") });
    last = nextEnd;
  });
  parts[0].sql = parts[0].sql.startsWith("SELECT") ? parts[0].sql : "SELECT " + parts[0].sql;
  return parts;
}

function uniqueRows(rows) {
  const seen = new Set(); const out = [];
  rows.forEach((row) => { const k = JSON.stringify(row); if (!seen.has(k)) { seen.add(k); out.push(row); } });
  return out;
}
function exceptRows(left, right) {
  const set = new Set(right.map((r) => JSON.stringify(r)));
  return uniqueRows(left.filter((r) => !set.has(JSON.stringify(r))));
}
function intersectRows(left, right) {
  const set = new Set(right.map((r) => JSON.stringify(r)));
  return uniqueRows(left.filter((r) => set.has(JSON.stringify(r))));
}
function eq(a, b) { return a !== null && b !== null && a === b; }
function cmpVal(a, b) {
  if (a === b) return 0;
  if (a === null) return -1;
  if (b === null) return 1;
  return a < b ? -1 : 1;
}

function seedDemo(db) {
  db.reset();
  db.execute("CREATE TABLE users (id INT, name TEXT, email TEXT, active BOOL)");
  db.execute("INSERT INTO users VALUES (1, 'Ada Lovelace', 'ada@example.com', true), (2, 'Grace Hopper', 'grace@example.com', false), (3, 'Alan Turing', 'alan@example.com', true)");
  db.execute("CREATE TABLE orders (id INT, user_id INT, total INT)");
  db.execute("INSERT INTO orders VALUES (10, 1, 120), (11, 1, 250), (12, 2, 40), (13, 3, 90)");
}

// Expand star projections after schema is known.
const _evalItem = evalItem;
evalItem = function (schema, rows, item) {
  if (item.star && !item.agg) return undefined;
  return _evalItem(schema, rows, item);
};

const _selectCore = MiniDB.prototype.selectCore;
MiniDB.prototype.selectCore = function (sql) {
  const parsed = parseSelect(sql);
  if (parsed.items.length === 1 && parsed.items[0].star && !parsed.items[0].agg) {
    const left = this.table(parsed.table);
    const names = parsed.joins.length
      ? left.columns.map((c) => `${left.name}.${c.name}`)
      : left.columns.map((c) => c.name);
    parsed.joins.forEach((join) => {
      this.table(join.table).columns.forEach((c) => names.push(`${join.table.toLowerCase()}.${c.name}`));
    });
    const rewritten = sql.replace(/SELECT\s+(DISTINCT\s+)?\*/i, `SELECT $1${names.join(", ")}`);
    return _selectCore.call(this, rewritten);
  }
  return _selectCore.call(this, sql);
};
