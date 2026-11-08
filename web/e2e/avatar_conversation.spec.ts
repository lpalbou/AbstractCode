import {test,expect,type Locator,type Page} from '@playwright/test';
import {openWorkspaceSection,openWorkflowInputs,closeWorkspaceDrawer} from './drawer_navigation';
const gateway=process.env.ABSTRACTCODE_E2E_GATEWAY_URL||'http://127.0.0.1:18781';
test('optional avatar keeps drafts, answers the real current question, and preserves final history',async({page},testInfo)=>{
 const errors:string[]=[];page.on('pageerror',e=>errors.push(e.message));
 // Warm the disposable principal host before exercising the app's short login timeout.
 const ready=await page.request.get(`${gateway}/api/gateway/bundles`,{headers:{Authorization:'Bearer abstractcode-e2e-only'},timeout:60000});
 expect(ready.ok()).toBe(true);
 const appOrigin=new URL(process.env.ABSTRACTCODE_E2E_URL||'http://127.0.0.1:18782').origin;
 await page.route('**/*',route=>new URL(route.request().url()).origin===appOrigin?route.continue():route.abort('blockedbyclient'));
 let start:any=null; const commands:any[]=[];
 page.on('request',r=>{if(r.method()==='POST'&&r.url().endsWith('/runs/start'))start=r.postDataJSON();if(r.method()==='POST'&&r.url().endsWith('/commands'))commands.push(r.postDataJSON());});
 await page.goto('/');
 await page.locator('#gateway-session-url').fill(gateway);
 await page.locator('#gateway-session-user').fill('web-tester');
 await page.locator('#gateway-session-token').fill('abstractcode-e2e-only');
 await page.getByRole('button',{name:'Sign in',exact:true}).click();
 await expect(page.getByRole('dialog',{name:'Gateway connection'})).toBeHidden();
 await expect(page.locator('.code-session-placeholder .af-session-card__open')).toHaveAttribute('aria-current','page');
 // The open row: a tone and a 1 px hairline (sidebar rows, 2026-11-08), never a 2 px frame.
 await expect(page.locator('.code-session-placeholder')).toHaveCSS('box-shadow', /inset.*\b1px|\b1px.*inset/);
 await page.locator('.code-session-placeholder').screenshot({path:testInfo.outputPath('new-conversation-selected.png')});
 await openWorkspaceSection(page,'Workflow');
 const picker=page.getByRole('combobox',{name:'Workflow',exact:true});await expect(picker).toBeEnabled();await picker.click();
 await page.getByRole('listbox',{name:'Workflow'}).getByRole('option').filter({has:page.locator('.af-workflow-picker__name',{hasText:/^Prompt structured$/})}).click();
 await closeWorkspaceDrawer(page);
 await openWorkflowInputs(page);
 await page.locator('.code-rail .af-rail__panel').getByLabel(/Ticket/).fill('avatar-test');
 await closeWorkspaceDrawer(page);
 const composer=page.locator('.code-conversation .pc-composer textarea');
 await composer.fill('Keep this draft');
 await expect(page.getByRole('region',{name:'Avatar conversation',exact:true})).toHaveCount(0);
 await page.getByRole('button',{name:'Avatar',exact:true}).click();
 await expect(composer).toHaveValue('Keep this draft');
 await expect(page.getByRole('region',{name:'Avatar conversation',exact:true})).toBeVisible();
 await composer.fill('Check the fictional lunch options');
 await page.getByRole('button',{name:'Send',exact:true}).click();
 await expect(page.getByRole('heading',{name:'A question for you',exact:true})).toBeVisible();
 expect(start?.conversation).toBe(true);
 await composer.fill('continue');await page.getByRole('button',{name:'Send',exact:true}).click();
 await expect.poll(()=>commands.some(c=>c.type==='resume'&&c.payload?.payload?.response==='continue')).toBe(true);
 await expect(page.locator('.pc-avatar-conversation__status')).toHaveText('Completed');
 await expect(page.locator('.pc-avatar-conversation__answer')).toContainText('structured-output');
 // No accent stripe on the answer: its leading border is no wider than its other sides.
 expect(await page.locator('.pc-avatar-conversation__answer .pc-chat-item').evaluate((el)=>{const s=getComputedStyle(el);return parseFloat(s.borderInlineStartWidth)<=parseFloat(s.borderInlineEndWidth);})).toBe(true);
 await expect(page.locator('.pc-avatar-conversation .af-voice-mic')).toHaveCount(0);
 await expect(page.locator('.code-conversation .pc-composer .af-voice-mic')).toHaveCount(1);
 await page.screenshot({path:testInfo.outputPath('avatar-code-desktop.png')});
 await page.getByRole('button',{name:'Conversation',exact:true}).click();
 await expect(page.getByRole('region',{name:'Avatar conversation',exact:true})).toHaveCount(0);
 await expect(page.locator('.code-conversation .pc-chat-thread')).toContainText('structured-output');
 expect(errors).toEqual([]);
});

test('avatar keeps the real approval and compact multiline composer visible',async({page},testInfo)=>{
 const ready=await page.request.get(`${gateway}/api/gateway/bundles`,{headers:{Authorization:'Bearer abstractcode-e2e-only'},timeout:60000});expect(ready.ok()).toBe(true);
 const appOrigin=new URL(process.env.ABSTRACTCODE_E2E_URL||'http://127.0.0.1:18782').origin;
 await page.route('**/*',route=>new URL(route.request().url()).origin===appOrigin?route.continue():route.abort('blockedbyclient'));
 const commands:any[]=[];page.on('request',r=>{if(r.method()==='POST'&&r.url().endsWith('/commands'))commands.push(r.postDataJSON());});
 await page.goto('/');await page.locator('#gateway-session-url').fill(gateway);await page.locator('#gateway-session-user').fill('web-tester');await page.locator('#gateway-session-token').fill('abstractcode-e2e-only');await page.getByRole('button',{name:'Sign in',exact:true}).click();await expect(page.getByRole('dialog',{name:'Gateway connection'})).toBeHidden();
 await openWorkspaceSection(page,'Workflow');const picker=page.getByRole('combobox',{name:'Workflow',exact:true});await expect(picker).toBeEnabled();await picker.click();await page.getByRole('listbox',{name:'Workflow'}).getByRole('option').filter({has:page.locator('.af-workflow-picker__name',{hasText:/^Delegated tool approval$/})}).click();await closeWorkspaceDrawer(page);
 await page.getByRole('button',{name:'Avatar',exact:true}).click();
 const composer=page.locator('.code-conversation .pc-composer');await expect(composer).toHaveClass(/pc-composer--compact/);
 const input=composer.getByRole('textbox',{name:'Message',exact:true});await input.fill('Review these notes\nKeep both options\nCompare access');await expect(input).toHaveValue(/\n.*\n/);
 await expect(input).toHaveCSS('outline-style','none');
 await composer.screenshot({path:testInfo.outputPath('code-compact-multiline.png'),animations:'disabled'});
 await page.getByRole('button',{name:'Send',exact:true}).click();
 const allow=page.getByRole('button',{name:'Allow',exact:true});await expect(allow).toBeVisible();
 await expect(page.locator('.pc-avatar-conversation__status')).toHaveText('Permission required');
 await expect(page.getByRole('heading',{name:'May I write to this file?',exact:true})).toBeVisible();
 await expect(page.getByText('The reason for this request isn’t available here.',{exact:true})).toBeVisible();
 await expect(page.locator('.pc-public-approval__target')).toHaveText(/(?:^|\/)fixture-tool-approval\.txt$/);
 await expect(page.locator('.pc-public-approval__action')).toHaveText('Write to a file — this can replace its contents');
 const publicRequest=page.locator('.pc-avatar-conversation__request');
 await expect(publicRequest).not.toContainText(/write_file|read_file|fetch_url|"arguments"|"content"|call_id|Allow for this chat|Allow all enabled tools/);
 await expect(publicRequest.locator('.pc-json-viewer')).toHaveCount(0);
 await expect(allow.locator('svg')).toHaveCount(1);await expect(page.getByRole('button',{name:'Deny',exact:true}).locator('svg')).toHaveCount(1);
 const visible=async(locator:any)=>locator.evaluate((el:Element)=>{const b=el.getBoundingClientRect();let top=0,bottom=innerHeight;for(let p=el.parentElement;p;p=p.parentElement){if(/auto|scroll|hidden|clip/.test(getComputedStyle(p).overflowY)){const r=p.getBoundingClientRect();top=Math.max(top,r.top);bottom=Math.min(bottom,r.bottom);}}return b.top>=top&&b.bottom<=bottom;});
 for(const viewport of[{width:1280,height:800},{width:1000,height:650},{width:390,height:844}]){
  await page.setViewportSize(viewport);
  await expect.poll(()=>visible(allow)).toBe(true);await expect.poll(()=>visible(page.locator('.pc-avatar-conversation__request .pc-workflow-interaction__prompt'))).toBe(true);
  await allow.click({trial:true});
  await expect(composer.getByRole('textbox',{name:'Message',exact:true})).toBeVisible();expect(commands.length).toBe(0);
  await page.screenshot({path:testInfo.outputPath(`avatar-public-code-fixture-${viewport.width}.png`),animations:'disabled'});
 }
 await page.getByRole('button',{name:'Review details',exact:true}).click();
 await expect(page.getByRole('region',{name:'Avatar conversation',exact:true})).toHaveCount(0);expect(commands).toEqual([]);
 await page.getByRole('button',{name:'Avatar',exact:true}).click();await expect(allow).toBeVisible();
 await allow.evaluate(element=>{(element as HTMLButtonElement).click();(element as HTMLButtonElement).click();});
 await expect.poll(()=>commands.filter(c=>c.type==='resume'&&c.payload?.payload?.approved===true).length).toBe(1);
 await expect(page.locator('.pc-avatar-conversation__status')).toHaveText('Completed');
 await page.getByRole('button',{name:'Conversation',exact:true}).click();await expect(composer).toHaveClass(/pc-composer--compact/);await expect(composer.getByRole('button',{name:'Attach file',exact:true})).toHaveCount(1);
});

async function publicTargetVisible(locator:Locator):Promise<boolean>{
 return locator.evaluate(element=>{const box=element.getBoundingClientRect();let top=0,bottom=innerHeight,left=0,right=innerWidth;for(let parent=element.parentElement;parent;parent=parent.parentElement){const style=getComputedStyle(parent),rect=parent.getBoundingClientRect();if(/auto|scroll|hidden|clip/.test(style.overflowY)){top=Math.max(top,rect.top);bottom=Math.min(bottom,rect.bottom);}if(/auto|scroll|hidden|clip/.test(style.overflowX)){left=Math.max(left,rect.left);right=Math.min(right,rect.right);}}return box.width>0&&box.height>0&&box.top>=top-1&&box.bottom<=bottom+1&&box.left>=left-1&&box.right<=right+1;});
}

async function startDelegatedApproval(page:Page):Promise<void>{
 const ready=await page.request.get(`${gateway}/api/gateway/bundles`,{headers:{Authorization:'Bearer abstractcode-e2e-only'},timeout:60000});expect(ready.ok()).toBe(true);
 const appOrigin=new URL(process.env.ABSTRACTCODE_E2E_URL||'http://127.0.0.1:18782').origin;
 await page.route('**/*',route=>new URL(route.request().url()).origin===appOrigin?route.continue():route.abort('blockedbyclient'));
 await page.goto('/');await page.locator('#gateway-session-url').fill(gateway);await page.locator('#gateway-session-user').fill('web-tester');await page.locator('#gateway-session-token').fill('abstractcode-e2e-only');await page.getByRole('button',{name:'Sign in',exact:true}).click();await expect(page.getByRole('dialog',{name:'Gateway connection'})).toBeHidden();
 await openWorkspaceSection(page,'Workflow');const picker=page.getByRole('combobox',{name:'Workflow',exact:true});await expect(picker).toBeEnabled();await picker.click();await page.getByRole('listbox',{name:'Workflow'}).getByRole('option').filter({has:page.locator('.af-workflow-picker__name',{hasText:/^Delegated tool approval$/})}).click();await closeWorkspaceDrawer(page);
 await page.getByRole('button',{name:'Avatar',exact:true}).click();
}

test('public webpage approval uses a deterministic served-view double with an exact bound why',async({page},testInfo)=>{
 // This tests the actual built Code host. Only its served replay view is doubled:
 // the real disposable Gateway fixture remains unchanged and no URL/model is read.
 const WHY='I want to compare these two restaurant pages to check their opening hours.';
 const URLS=['https://restaurant.example/menu','https://riverside.example/opening-hours'];
 const decisions:any[]=[];const errors:string[]=[];page.on('pageerror',e=>errors.push(e.message));
 await startDelegatedApproval(page);
 const substitute=(view:any)=>{
  if(view.interaction?.kind!=='tool_approval')return view;
  const interaction=view.interaction;
  const calls=URLS.map((url,index)=>({id:`test-page-${index}`,call_id:`test-page-${index}`,name:'fetch_url',arguments:{url}}));
  return {...view,interaction:{...interaction,tool_calls:calls,details:{...interaction.details,public_action:{text:WHY,run_id:interaction.run_id,update_id:'test-double-public-why',iteration:2,call_ids:calls.map(call=>call.call_id)}}}};
 };
 await page.route('**/api/gateway/runs/*/view',async route=>{
  const response=await route.fetch();expect(response.ok()).toBe(true);const body=await response.json();await route.fulfill({response,json:substitute(body)});
 });
 await page.route('**/api/gateway/runs/*/view/stream',async route=>{
  const viewUrl=route.request().url().replace(/\/view\/stream(?:\?.*)?$/,'/view');
  let view:any=null;
  for(let attempt=0;attempt<30;attempt++){
   const response=await route.fetch({url:viewUrl,timeout:5000});expect(response.ok()).toBe(true);view=await response.json();
   if(view.interaction?.kind==='tool_approval')break;
   await new Promise(resolve=>setTimeout(resolve,100));
  }
  expect(view.interaction?.kind).toBe('tool_approval');
  const frame=substitute(view);await route.fulfill({contentType:'text/event-stream',body:`event: view\nid: ${frame.cursor.token}\ndata: ${JSON.stringify(frame)}\n\n`});
 });
 await page.route('**/api/gateway/commands',async route=>{
  // A decision receipt is a test double too; never approve the fixture's different write action.
  decisions.push(route.request().postDataJSON());await route.fulfill({status:202,json:{accepted:true}});
 });
 await page.locator('.code-conversation .pc-composer textarea').fill('Compare the two restaurant pages');
 await page.getByRole('button',{name:'Send',exact:true}).click();
 const allow=page.getByRole('button',{name:'Allow',exact:true});
 await expect(page.getByRole('heading',{name:'May I read these 2 webpages?',exact:true})).toBeVisible();
 const request=page.locator('.pc-avatar-conversation__request');
 await expect(request).toContainText(WHY);
 await expect(request).not.toContainText(/fetch_url|write_file|read_file|DO_NOT_RENDER_JSON|"arguments"|"url"|call_id|Allow for this chat|Allow all enabled tools/);
 await expect(request.locator('.pc-json-viewer')).toHaveCount(0);
 await expect(allow.locator('svg')).toHaveCount(1);await expect(page.getByRole('button',{name:'Deny',exact:true}).locator('svg')).toHaveCount(1);
 for(const viewport of[{width:1280,height:800},{width:1000,height:650},{width:390,height:844}]){
  await page.setViewportSize(viewport);
  await page.screenshot({path:testInfo.outputPath(`avatar-public-code-pages-${viewport.width}.png`),animations:'disabled'});
  for(const locator of[page.getByRole('heading',{name:'May I read these 2 webpages?',exact:true}),page.getByText(WHY,{exact:true}),...URLS.map(url=>page.getByText(url,{exact:true})),allow,page.getByRole('button',{name:'Deny',exact:true}),page.getByRole('button',{name:'Review details',exact:true})]){
   const label=await locator.innerText();
   await expect.poll(()=>publicTargetVisible(locator),{message:`Readable ${label} at ${viewport.width}x${viewport.height}`}).toBe(true);
  }
  await allow.click({trial:true});expect(decisions).toEqual([]);
  await page.screenshot({path:testInfo.outputPath(`avatar-public-code-pages-${viewport.width}.png`),animations:'disabled'});
 }
 await page.getByRole('button',{name:'Deny',exact:true}).click();
 await expect.poll(()=>decisions.length).toBe(1);expect(decisions[0].type).toBe('resume');expect(decisions[0].payload?.payload?.approved).toBe(false);
 expect(decisions.some(command=>command.payload?.payload?.approved===true)).toBe(false);expect(errors).toEqual([]);
});
