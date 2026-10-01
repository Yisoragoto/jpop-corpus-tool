
const search = document.getElementById('ts');
const tbody  = document.getElementById('tb');
search.addEventListener('input', () => {
  const q = search.value.toLowerCase();
  for (const tr of tbody.rows)
    tr.classList.toggle('hidden', q.length > 0 && !(tr.dataset.s||'').includes(q));
});
let sc = 0, sa = true;
function sortBy(ci){
  sa = sc===ci ? !sa : (sc=ci, true);
  document.querySelectorAll('th').forEach((th,i)=>{
    th.classList.remove('sort-asc','sort-desc');
    if(i===sc) th.classList.add(sa?'sort-asc':'sort-desc');
  });
  const rows=[...tbody.rows].sort((a,b)=>{
    const av=a.cells[ci].textContent.replace(/[,，—✅⬜\s]/g,'');
    const bv=b.cells[ci].textContent.replace(/[,，—✅⬜\s]/g,'');
    const an=parseFloat(av),bn=parseFloat(bv);
    const c=(!isNaN(an)&&!isNaN(bn))?an-bn:av.localeCompare(bv,'ja');
    return sa?c:-c;
  });
  rows.forEach(r=>tbody.appendChild(r));
}
document.querySelectorAll('th').forEach((th,i)=>th.addEventListener('click',()=>sortBy(i)));
