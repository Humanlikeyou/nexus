import { chromium } from '/Users/thevisualdude/projects/nexus-amara/node_modules/playwright/index.mjs';
import fs from 'fs';
import {readdirSync} from 'fs';const require_dir=()=>readdirSync(process.env.HOME+'/Library/Caches/ms-playwright/chromium_headless_shell-1243').find(x=>x.startsWith('chrome-headless'));
const base=process.argv[2];const dir=process.argv[3];
const b=await chromium.launch({executablePath:process.env.HOME+'/Library/Caches/ms-playwright/chromium_headless_shell-1243/'+require_dir()+'/chrome-headless-shell',args:['--use-gl=angle','--use-angle=swiftshader','--enable-unsafe-swiftshader','--ignore-gpu-blocklist']});
const errs=[];
for(const theme of ['dark','light']){
  const ctx=await b.newContext({viewport:{width:1360,height:900},deviceScaleFactor:1,colorScheme:theme});
  const page=await ctx.newPage();
  page.on('console',m=>{if(['error','warning'].includes(m.type()))errs.push(theme+': '+m.type()+' '+m.text());});
  page.on('pageerror',e=>errs.push(theme+': pageerror '+e.message));
  await page.route('**/three.module.min.js',r=>r.fulfill({path:'/Users/thevisualdude/projects/nexus-amara/node_modules/three/build/three.module.min.js',contentType:'text/javascript',headers:{'access-control-allow-origin':'*'}}));
  await page.goto(base+'/index.html?theme='+theme);
  await page.waitForFunction('window.__ready===true',{timeout:20000});
  await page.waitForTimeout(2500);
  if(theme==='dark'||theme==='light'){
    // per panel
    const ids=['panel-1','panel-2','panel-3','panel-4','panel-5','panel-6','panel-7','panel-8'];
    for(let i=0;i<ids.length;i++){
      const el=page.locator('#'+ids[i]);await el.scrollIntoViewIfNeeded();await page.waitForTimeout(1300);
      await el.screenshot({path:`${dir}/shot-${i+1}${theme==='light'?'-light':''}.png`});
    }
    // full page via tall viewport
    const h=await page.evaluate('document.documentElement.scrollHeight');
    await page.setViewportSize({width:1360,height:Math.min(h,9000)});await page.waitForTimeout(2000);
    await page.screenshot({path:`${dir}/shot-${theme}.png`});
  }
  await ctx.close();
}
console.log(JSON.stringify(errs,null,1));
await b.close();
