const $ = (s) => document.querySelector(s);
const hue = (i) => `hsl(${(i * 67) % 360} 55% 45%)`;
const I = {
  shuffle: '<path d="M16 3h5v5M4 20 21 3M21 16v5h-5M15 15l6 6M4 4l5 5"/>',
  play: '<path d="M7 4v16l13-8z"/>',
  search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/>',
  more: '<circle cx="12" cy="5" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="12" cy="19" r="1"/>',
  up: '<path d="M12 19V5M5 12l7-7 7 7"/>',
  note: '<path d="M9 18V5l11-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="17" cy="16" r="3"/>',
  disc: '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="2"/>',
  list: '<path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  check: '<path d="m5 12 5 5 9-10"/>',
  back: '<path d="M19 12H5M12 19l-7-7 7-7"/>',
};
const svg = (k) => `<svg>${I[k]}</svg>`;

// demo data
const albums = [['I feel', '(G)I-DLE', 2023], ['I love', '(G)I-DLE', 2022], ['009 Sound System', '009 Sound System', 2009], ['This Is', '300Skulls', 2021], ['THIS IS GLOOM', '300Skulls', 2022], ['Lollita', 'A.R. Kane', 1987], ['MY WORLD', 'aespa', 2023], ['Savage', 'aespa', 2021], ['Living in Darkness', 'Agent Orange', 1981], ['Winks & Kisses', 'Airiel', 2007], ['Selected Ambient Works 85–92', 'Aphex Twin', 1992], ['It Never Rains in Southern California', 'Albert Hammond', 1972], ['Amnesiac', 'Radiohead', 2001], ['Hail to the Thief', 'Radiohead', 2003], ['In Rainbows', 'Radiohead', 2007], ['Rusty', 'Rodan', 1994], ['13 Songs', 'Fugazi', 1989], ['Repetition', 'Unwound', 1996]]
  .map(([t, a, y], i) => ({ t, a, y, c: hue(i), i }));
const saw = [['Xtal', '4:54'], ['Tha', '9:01'], ['Pulsewidth', '3:47'], ['Ageispolis', '5:21'], ['i', '1:13'], ['Green Calx', '6:05'], ['Heliosphan', '4:54'], ['We Are the Music Makers', '7:44'], ['Schottkey 7th Path', '5:09'], ['Ptolemy', '7:14'], ['Hedphelym', '6:02'], ['Delphium', '5:36'], ['Actium', '7:35']];
const words = ['Shiner', 'Gauge', 'Hollow', 'Static', 'Winter', 'Glass', 'Signal', 'Drift', 'Ember', 'Tide', 'Paper', 'Echo'];
for (const al of albums) {
  al.tracks = al.a === 'Aphex Twin'
    ? saw.map(([t, d], n) => ({ t, d, n: n + 1, disc: 1, al }))
    : Array.from({ length: 6 + (al.i % 5) }, (_, n) => ({ t: words[(al.i + n * 5) % words.length] + (n > 9 ? ' II' : ''), d: `${2 + ((al.i + n) % 6)}:${String((al.i * 7 + n * 13) % 60).padStart(2, '0')}`, n: n + 1, disc: n > 7 ? 2 : 1, al }));
}
albums.at(-1).tracks.push({ t: 'Radio Grace', d: '6:18', n: 9, disc: 1, al: albums.at(-1) });
const allTracks = albums.flatMap((a) => a.tracks);
const secs = (d) => d.split(':').reduce((m, s) => m * 60 + +s, 0);
const length = (ts) => { const s = ts.reduce((n, t) => n + secs(t.d), 0); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; };
const playlists = ['Late night', 'Running', 'Noise rock picks', 'Downbeat', 'Car', 'Favourites 2025', 'Saturday', 'Sleep']
  .map((t, i) => ({ t, a: `${8 + i * 3} songs`, c: hue(i + 30), i, tracks: allTracks.filter((_, k) => (k * 7 + i) % 9 === 0) }));
const recent = (k) => allTracks.filter((_, n) => (n * 5 + k) % 7 === 0);
const recentAlbums = (k) => albums.filter((_, n) => (n + k) % 3 === 0);

const cats = [
  ['All Songs', '#6f8fd8', 'note', { v: 'songs', src: 'all' }],
  ['Albums', '#5a55b8', 'disc', { v: 'coll', kind: 'albums' }],
  ['Playlists', '#7a4638', 'list', { v: 'coll', kind: 'playlists' }],
  ['Recently Played', '#3f3d8a', 'play', { v: 'songs', src: 'played', show: 'songs' }],
  ['Recently Added', '#4e6043', 'plus', { v: 'songs', src: 'added', show: 'songs' }],
];
const catOf = (v) => cats.find((c) => c[3].kind === v.kind && c[3].src === v.src && c[3].v === v.v);

// view state
let stack = [{ v: 'library' }], np = null, menu = false, engines = false, sheet = false;
const cur = () => stack.at(-1);
const opts = {};
// shortcut: one options set per kind of view, the real app would keep them per view in settings
const optKey = (v) => v.v === 'library' ? 'library' : v.v === 'coll' || v.show === 'albums' ? 'coll' : v.src === 'album' ? 'album' : 'songs';
const getOpts = (v) => opts[optKey(v)] ??= optKey(v) === 'coll'
  ? { mode: 'grid', cols: 3, titles: true, artists: true }
  : { mode: 'list', cols: 3, titles: true, artists: true };
const go = (v) => { stack.push(v); np = null; menu = false; render(); $('#view').scrollTop = 0; };
const back = () => { if (stack.length > 1) { stack.pop(); render(); } };
const title = (v) => v.v === 'library' ? 'Library' : v.v === 'search' ? 'Search' : v.v === 'settings' ? 'Settings' : v.src === 'album' ? albums[v.i].t : v.src === 'playlist' ? playlists[v.i].t : catOf(v)?.[0];

// renderers
const toolbar = (select, extra = '') => `<div class="toolbar">
  <button class="iconbtn" aria-label="Shuffle">${svg('shuffle')}</button>
  <button class="iconbtn" aria-label="Play">${svg('play')}</button>
  <button class="iconbtn" aria-label="Search">${svg('search')}</button>
  ${select ? '<button class="textbtn">Select</button>' : ''}${extra}
  <button class="iconbtn more" data-sheet aria-label="More">${svg('more')}</button></div>`;

function collBody(items, o, open) {
  if (o.mode === 'grid') return `<div class="grid" style="--cols:${o.cols}">${items.map((x) =>
    `<button class="tile" data-open="${open}:${x.i}"><div class="art" style="--c:${x.c}"></div>${o.titles ? `<div class="t">${x.t}</div>` : ''}${o.artists ? `<div class="a">${x.a}</div>` : ''}</button>`).join('')}</div>`;
  return items.map((x) => `<button class="row" data-open="${open}:${x.i}">${o.mode === 'covers' ? `<div class="art" style="--c:${x.c}"></div>` : ''}<div class="tt"><div class="t">${x.t}</div><div class="s">${x.a}</div></div></button>`).join('');
}

function songBody(tracks, o, inAlbum) {
  if (o.mode === 'grid') return `<div class="grid" style="--cols:${o.cols}">${tracks.map((t) =>
    `<button class="tile"><div class="art" style="--c:${t.al.c}"></div>${o.titles ? `<div class="t">${t.t}</div>` : ''}${o.artists ? `<div class="a">${t.al.a}</div>` : ''}</button>`).join('')}</div>`;
  return tracks.map((t, k) => `<button class="track ${t.t === 'Pulsewidth' ? 'playing' : ''}">${o.mode === 'covers' ? `<div class="art" style="--c:${t.al.c}"></div>` : `<span class="n">${inAlbum ? t.n : k + 1}</span>`}
    <span class="tt"><span class="name">${t.t}</span>${inAlbum ? '' : `<span class="s">${t.al.a}</span>`}</span>
    <span class="info">${inAlbum ? `disc ${t.disc} · ` : ''}${t.d} · opus</span></button>`).join('');
}

function render() {
  const v = cur(), o = getOpts(v), prev = stack.at(-2);
  const backBtn = prev ? `<button class="back" data-back>${svg('up')}${title(prev)}</button>` : '';
  let html;
  if (v.v === 'library') {
    html = `<div class="lib-head"><h1>Library</h1><button class="iconbtn" data-sheet aria-label="More">${svg('more')}</button></div>
      ${cats.map(([n, c, ic], k) => `<button class="cat" data-cat="${k}"><span class="dot" style="--c:${c}">${svg(ic)}</span>${n}</button>`).join('')}`;
  } else if (v.v === 'search') {
    html = `<label class="searchbox">${svg('search')}<input id="q" value="${search.q}" placeholder="Search your library" autocomplete="off"><button data-clear aria-label="Clear">✕</button></label>
      <div class="chiprow">${FILTERS.map((f) => `<button data-filter="${f}" class="${search.f === f ? 'on' : ''}">${f}</button>`).join('')}</div>
      <div class="search-tb">${toolbar(true).replace(/<button class="iconbtn" aria-label="Search">.*?<\/button>/s, '')}</div>
      <div id="results">${results()}</div>`;
  } else if (v.v === 'settings') {
    html = `<div class="set-head"><button data-back aria-label="Back">${svg('back')}</button><h1>Settings</h1><button aria-label="Search settings">${svg('search')}</button></div>
      ${GROUPS.map(([t, s, c, ic]) => `<button class="set-group"><span class="ic" style="--c:${c}"><svg>${ic}</svg></span><span><div class="t">${t}</div><div class="s">${s}</div></span></button>`).join('')}`;
  } else if (v.src === 'album' || v.src === 'playlist') {
    const x = v.src === 'album' ? albums[v.i] : playlists[v.i];
    html = `<div class="hero art" style="--c:${x.c}"></div>
      <div class="album-meta"><h1>${x.t}</h1><p>${v.src === 'album' ? `${x.a} · ${x.y}` : x.a}</p></div>
      ${toolbar(true)}${songBody(x.tracks, o, v.src === 'album')}`;
  } else {
    const [n, c, ic] = catOf(v);
    const seg = v.show ? `<div class="seg">${['songs', 'albums'].map((s) => `<button data-show="${s}" class="${v.show === s ? 'on' : ''}">${s[0].toUpperCase() + s.slice(1)}</button>`).join('')}</div>` : '';
    const body = v.v === 'coll'
      ? collBody(v.kind === 'albums' ? albums : playlists, o, v.kind === 'albums' ? 'album' : 'playlist')
      : v.show === 'albums' ? collBody(recentAlbums(v.src.length), o, 'album')
      : songBody(v.src === 'all' ? allTracks : recent(v.src.length), o, false);
    html = `${backBtn}<div class="title-row"><span class="dot" style="--c:${c}">${svg(ic)}</span><h1>${n}</h1></div>
      ${toolbar(v.v === 'songs' && v.show !== 'albums', seg)}${body}`;
  }
  const view = $('#view');
  view.innerHTML = html;
  view.classList.toggle('flush', v.src === 'album' || v.src === 'playlist');

  document.querySelectorAll('.nav [data-go]').forEach((b) => b.classList.toggle('on', stack[0].v === b.dataset.go));
  $('#np').className = 'np' + (np ? ' on ' + np : '');
  $('#bar').classList.toggle('open', menu);
  $('#engines').hidden = !engines;
  $('#menu-items').hidden = engines;
  $('#scrim').classList.toggle('on', menu || sheet);
  $('#sheet').classList.toggle('on', sheet);
  if (sheet) $('#sheet').innerHTML = sheetHtml(v, o);
  document.querySelectorAll('#jump button').forEach((b) => b.classList.toggle('on', b.dataset.k === jumpKey()));
}

function sheetHtml(v, o) {
  const row = (label, ctl) => `<div class="opt"><span>${label}</span>${ctl}</div>`;
  const toggle = (k) => `<button class="switch ${o[k] ? 'on' : ''}" data-toggle="${k}" aria-label="${k}"></button>`;
  const actions = (list) => list.map((a) => `<button class="item">${a}</button>`).join('');
  if (v.v === 'library') return `<h3>Library</h3>${actions(['Add folder', 'Rescan library', 'Add to launcher'])}`;
  const coll = optKey(v) === 'coll';
  const modes = coll ? [['grid', 'Grid'], ['list', 'List'], ['covers', 'List + covers']] : [['list', 'List'], ['covers', 'List + covers'], ['grid', 'Grid']];
  return `<h3>${title(v)}</h3>
    ${row('View', `<div class="seg">${modes.map(([k, n]) => `<button data-mode="${k}" class="${o.mode === k ? 'on' : ''}">${n}</button>`).join('')}</div>`)}
    ${o.mode === 'grid' ? row('Columns', `<div class="step"><button data-cols="-1">−</button><b>${o.cols}</b><button data-cols="1">+</button></div>`)
      + row('Show titles', toggle('titles')) + row('Show artists', toggle('artists')) : ''}
    ${row('Sort by', `<span class="muted">${coll ? 'Artist' : v.src === 'album' ? 'Track number' : 'Title'} ›</span>`)}
    ${actions(['Add to launcher', 'Rescan library'])}`;
}

// search
const FILTERS = ['All', 'Songs', 'Albums', 'Artists', 'Album Artists', 'Folders', 'Playlists'];
const search = { q: 'rad', f: 'All' };
const albumRow = (al) => `<button class="arow" data-open="album:${al.i}"><div class="art" style="--c:${al.c}"></div><span><div class="t">${al.t}</div><div class="a">${al.a}</div><div class="m">${al.tracks.length} songs · ${length(al.tracks)} · ${al.y}</div></span></button>`;
const songRow = (t) => `<button class="track"><div class="art" style="--c:${t.al.c}"></div><span class="tt"><span class="name">${t.t}</span><span class="s">${t.al.a}</span></span><span class="info">opus · ${t.d}</span></button>`;
const nameRow = (name, sub, c, open = '') => `<button class="row" ${open}><span class="dot" style="--c:${c}">${name[0]}</span><div class="tt"><div class="t">${name}</div><div class="s">${sub}</div></div></button>`;
function results() {
  const q = search.q.trim().toLowerCase(), has = (s) => s.toLowerCase().includes(q);
  if (!q) return '<p class="empty">Search songs, albums, artists, folders and playlists</p>';
  const artists = [...new Set(albums.map((a) => a.a))].filter(has);
  const als = albums.filter((a) => has(a.t) || has(a.a)), songs = allTracks.filter((t) => has(t.t)).slice(0, 30);
  const artistRows = () => artists.map((a, k) => nameRow(a, `${albums.filter((x) => x.a === a).length} albums`, hue(k + 5))).join('');
  const sec = (name, body) => body ? `<div class="sub">${name}</div>${body}` : '';
  let html;
  switch (search.f) {
    case 'Songs': html = songs.map(songRow).join(''); break;
    case 'Albums': html = als.map(albumRow).join(''); break;
    case 'Artists': case 'Album Artists': html = artistRows(); break;
    case 'Folders': html = als.map((a, k) => nameRow(`${a.a}/${a.t}`, `${a.tracks.length} files`, hue(k), `data-open="album:${a.i}"`)).join(''); break;
    case 'Playlists': html = playlists.filter((p) => has(p.t)).map((p) => nameRow(p.t, p.a, p.c, `data-open="playlist:${p.i}"`)).join(''); break;
    default: {
      // best match: the first matching artist leads with their albums
      const best = artists[0];
      html = (best ? `<div class="best"><span class="muted">artist</span><h2>${best}</h2></div>${albums.filter((a) => a.a === best).map(albumRow).join('')}` : '')
        + sec('Albums', als.filter((a) => a.a !== best).map(albumRow).join(''))
        + sec('Songs', songs.map(songRow).join(''));
    }
  }
  return html || '<p class="empty">Nothing matches</p>';
}
document.addEventListener('input', (e) => { if (e.target.id === 'q') { search.q = e.target.value; $('#results').innerHTML = results(); } });

// settings: the app's own groups (SETTINGS_GROUPS), keyboard left out on phones
const GROUPS = [
  ['Interface', 'Theme, accent, layouts, album art, language', '#8b7cf6', '<rect x="3" y="3" width="18" height="18" rx="3"/><path d="M3 9h18M9 21V9"/>'],
  ['Library', 'Music folders, rescan, sort orders', '#5b8bd6', '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>'],
  ['Playback', 'Output, replay gain, gapless, queue', '#d2618a', '<path d="M11 5 6 9H2v6h4l5 4zM15.5 8.5a5 5 0 0 1 0 7M19 5a10 10 0 0 1 0 14"/>'],
  ['Gestures', 'Swipes, taps and what they do', '#c9a24a', '<path d="M9 11V5a2 2 0 0 1 4 0v6M13 10a2 2 0 0 1 4 0v2M17 11a2 2 0 0 1 4 0v4a7 7 0 0 1-7 7h-1a7 7 0 0 1-6-3l-3-5a2 2 0 0 1 3-2l2 2"/>'],
  ['Integrations', 'Scrobbling, lyrics lookup, media controls', '#4fa58a', '<path d="M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7"/>'],
  ['Devices', 'Engines, pairing, LAN, MPD', '#d8834a', '<rect x="5" y="2" width="14" height="20" rx="2"/><path d="M12 18h.01"/>'],
  ['Technical', 'Logs, caches, developer options', '#8b8b94', '<path d="m8 9-4 3 4 3M16 9l4 3-4 3M14 5l-4 14"/>'],
  ['About', 'Version, changelog, licences', '#6f8fd8', '<circle cx="12" cy="12" r="9"/><path d="M12 16v-4M12 8h.01"/>'],
];

// lyrics: placeholder text, only the current line is highlighted
const LYRICS = [['A line of placeholder lyrics here,', 'standing in for the words of the song.', 'Another made-up line to read,', 'and one more to carry it along.'], ['This verse is the one being sung,', 'and this is the current line.'], ['These lines are still to come,', 'dimmed until the singer gets there.', 'Tapping a line would seek to it,', 'when lyrics are synced.'], ['A final short stanza', 'to fill out the page.']];
$('#lyrics').innerHTML = LYRICS.map((st, s) => st.map((l, k) => `<p class="${s === 1 && k === 1 ? 'cur' : ''}">${l}</p>`).join('')).join('<div class="gap"></div>');

// jump buttons
const jumps = {
  library: () => { stack = [{ v: 'library' }]; },
  albums: () => { stack = [{ v: 'library' }, cats[1][3]]; },
  album: () => { stack = [{ v: 'library' }, cats[1][3], { v: 'songs', src: 'album', i: 10 }]; },
  playlists: () => { stack = [{ v: 'library' }, cats[2][3]]; },
  playlist: () => { stack = [{ v: 'library' }, cats[2][3], { v: 'songs', src: 'playlist', i: 2 }]; },
  all: () => { stack = [{ v: 'library' }, cats[0][3]]; },
  recent: () => { stack = [{ v: 'library' }, { ...cats[4][3] }]; },
  search: () => { stack = [{ v: 'search' }]; },
  settings: () => { stack = [{ v: 'library' }, { v: 'settings' }]; },
  menu: () => { menu = true; },
  engines: () => { menu = true; engines = true; },
  np: () => { np = 'np'; }, lyrics: () => { np = 'lyrics'; }, half: () => { np = 'half'; }, full: () => { np = 'full'; },
};
const jumpNames = { library: 'Library', albums: 'Albums', album: 'Album', playlists: 'Playlists', playlist: 'Playlist', all: 'All Songs', recent: 'Recently Added', search: 'Search', settings: 'Settings', menu: 'Menu', engines: 'Engines', np: 'Now playing', lyrics: 'Lyrics', half: 'Half queue', full: 'Full queue' };
const jumpKey = () => engines ? 'engines' : menu ? 'menu' : np ?? Object.keys(jumps).slice(0, 9).find((k) => { const t = cur(); return { library: t.v === 'library', albums: t.kind === 'albums', album: t.src === 'album', playlists: t.kind === 'playlists', playlist: t.src === 'playlist', all: t.src === 'all', recent: t.src === 'added', search: t.v === 'search', settings: t.v === 'settings' }[k]; });
$('#jump').innerHTML = Object.entries(jumpNames).map(([k, n]) => `<button data-k="${k}">${n}</button>`).join('');

let swiped = false;
document.addEventListener('click', (e) => {
  if (swiped) { swiped = false; return; }
  const t = e.target.closest('button, .scrim, .conn');
  if (!t) return;
  const d = t.dataset, v = cur(), o = getOpts(v);
  if (d.k) { menu = engines = sheet = false; np = null; jumps[d.k](); }
  else if (d.cat) return go({ ...cats[d.cat][3] });
  else if (d.open) { const [src, i] = d.open.split(':'); return go({ v: 'songs', src, i: +i }); }
  else if ('back' in d) return back();
  else if (d.push) return go({ v: d.push });
  else if (d.filter) search.f = d.filter;
  else if ('clear' in d) { search.q = ''; render(); return $('#q').focus(); }
  else if (d.go) { stack = [{ v: d.go }]; np = null; menu = false; }
  else if (d.np) { np = d.np; menu = false; }
  else if (d.show) v.show = d.show;
  else if ('sheet' in d) sheet = true;
  else if (d.mode) o.mode = d.mode;
  else if (d.cols) o.cols = Math.min(6, Math.max(2, o.cols + +d.cols));
  else if (d.toggle) o[d.toggle] = !o[d.toggle];
  else if (t.id === 'burger') { menu = !menu; engines = false; }
  else if (t.classList.contains('conn')) engines = !engines;
  else if (t.id === 'scrim') menu = engines = sheet = false;
  else return;
  render();
});

// swipe right goes back a level; swipe down on now playing steps the sheet back
let p0 = null;
$('#view').addEventListener('pointerdown', (e) => { p0 = [e.clientX, e.clientY]; });
$('#view').addEventListener('pointerup', (e) => {
  if (p0 && e.clientX - p0[0] > 80 && Math.abs(e.clientY - p0[1]) < 60 && stack.length > 1) { swiped = true; back(); }
  p0 = null;
});
let y0 = null;
$('#np').addEventListener('pointerdown', (e) => { y0 = $('#np').scrollTop === 0 ? e.clientY : null; });
$('#np').addEventListener('pointerup', (e) => {
  if (y0 === null || e.clientY - y0 < 80) return;
  swiped = true;
  np = { full: 'half', half: 'np', lyrics: 'np', np: null }[np]; render();
});

// queue sheet content
const qrow = ([t, a, d], c, cur) =>
  `<div class="qrow ${cur ? 'cur' : ''}"><div class="art" style="--c:${c}"></div><div class="tt"><div class="t">${t}</div><div class="s">${a} · ${d}</div></div><span class="grip"><svg><path d="M5 9h14M5 15h14"/></svg></span></div>`;
$('#queue').innerHTML = [['Bible Silver Corner', 'Rodan', '6:53'], ['Shiner', 'Rodan', '2:39'], ['The Everyday World of Bodies', 'Rodan', '11:56'], ['Jungle Jim', 'Rodan', '7:32'], ['Gauge', 'Rodan', '7:18'], ['Tooth-Fairy Retribution Manifesto', 'Rodan', '6:29']]
  .map((r, i) => qrow(r, '#c8642c', i === 0)).join('');
$('#after').innerHTML = [['Darjeeling', 'Rodan', '4:10'], ['Notres Langues Nous Trompes', 'Saetia', '2:38'], ['I\'m So Tired', 'Fugazi', '1:59'], ['Usual Dosage', 'Unwound', '5:14']]
  .map((r, i) => qrow(r, hue(i + 20))).join('');

render();
