package com.yefeng.majmax.hookprobe.manager

import android.app.Instrumentation
import android.os.Bundle
import org.json.JSONArray
import org.json.JSONObject

/** Runs against Android's real JSONObject, clock supplied explicitly; no game input. */
class AutoDiscardInstrumentation : Instrumentation() {
    private var controllerOnly = false
    private var importFile: String? = null
    private var verifySelectedModels = false
    override fun onCreate(arguments: Bundle?) {
        controllerOnly = arguments?.getString("controllerOnly") == "true"
        importFile = arguments?.getString("importFile")
        verifySelectedModels = arguments?.getString("verifySelectedModels") == "true"
        super.onCreate(arguments); start()
    }
    override fun onStart() {
        val output = Bundle()
        runCatching {
            if (verifySelectedModels) {
                val before = targetContext.getSharedPreferences("onnx-policy-models", 0).all.toMap()
                val unattended = targetContext.getSharedPreferences("unattended", 0).all.toMap()
                OnnxModelStore.initialize(targetContext)
                val results = listOf(3,4).filter { OnnxModelStore.selectedCustom(targetContext,it) }
                    .map { OnnxModelStore.selfTest(targetContext,it) }
                check(results.isNotEmpty()) { "No selected custom models to verify" }
                check(before == targetContext.getSharedPreferences("onnx-policy-models", 0).all)
                check(unattended == targetContext.getSharedPreferences("unattended", 0).all)
                output.putString("stream", results.joinToString("\n", "PASS selected models; preferences unchanged\n", "\n"))
                finish(-1,output)
                return
            }
            importFile?.let { path ->
                val file = java.io.File(path).canonicalFile
                check(file.parentFile == targetContext.cacheDir.canonicalFile) { "Use a model in the target app cache" }
                check(AiNative.loadMortalPth(3, file.absolutePath, false) == 3) { "Expected a valid sanma checkpoint" }
                val prefs = targetContext.getSharedPreferences("onnx-policy-models", 0)
                val fourBefore = prefs.all.filterKeys { it.endsWith("_4") }
                val unattendedBefore = targetContext.getSharedPreferences("unattended", 0).all.toMap()
                val policy = OnnxModelStore.import(targetContext, android.net.Uri.fromFile(file))
                check(policy.players == 3 && policy.format == "mortal-pth")
                val result = OnnxModelStore.selfTest(targetContext, 3)
                check(OnnxModelStore.activate(3) == 0)
                check(OnnxModelStore.selectedCustom(targetContext, 3))
                check(prefs.all.filterKeys { it.endsWith("_4") } == fourBefore)
                check(targetContext.getSharedPreferences("unattended", 0).all == unattendedBefore)
                output.putString("stream", "PASS import ${policy.name}: ${policy.bytes} bytes, ${policy.sha256}\n$result\nPASS activation; four-player and unattended settings preserved\n")
                finish(-1, output)
                return
            }
            var tests = 0
            fun test(name: String, run: (AutoDiscardController, () -> Unit) -> Unit) {
                var time = 0L
                val gate = AutoDiscardController({}, { time }, { 0L })
                gate.invalidate("test reset")
                run(gate) { time += 12_001 }
                tests++
                sendStatus(0, Bundle().apply { putString("stream", "PASS $name\n") })
            }
            fun live(revision: Int = 1, kind: String = "discard") = JSONObject("""
                {"status":"live","canAct":true,"connection":"7","revision":$revision,
                 "sourceGeneration":3,"sourceSequence":8,"seat":0,"players":4,"hand":["5mr"],
                 "legalOperations":[{"type":1,"combination":[]}],
                 "autoContext":{"riichi":false},
                 "recommendations":[{"kind":"$kind","tile":"5mr","tsumogiri":false}]}
            """.trimIndent())
            fun ack(id: String, state: String) = JSONArray().put(JSONObject().put("id",id).put("state",state)).toString()
            test("default off and explicit enable") { gate, _ ->
                gate.observe(live()); check(gate.poll(null) == null)
                gate.toggle(); val command = JSONObject(checkNotNull(gate.poll(null)))
                check(command.getString("gameTile") == "0m")
                check(!command.getBoolean("tsumogiri"))
            }
            test("arming before sync never sends an input") { gate, _ ->
                gate.observe(live().put("status","demo")); gate.toggle(); check(gate.poll(null) == null)
                gate.observe(live()); gate.invalidate("disconnected"); gate.toggle(); check(gate.poll(null) == null)
            }
            test("new capture session invalidates queued input") { gate, _ ->
                gate.observe(live()); gate.toggle(); check(gate.poll(null) != null)
                gate.invalidate("new session"); check(gate.poll(null) == null && AutoDiscardState.mutable.value.enabled)
                gate.observe(live().put("sourceGeneration",4)); check(gate.poll(null) != null)
            }
            test("manual operation pauses") { gate, _ ->
                gate.observe(live()); gate.toggle()
                gate.poll(ack("", "manual")); check(gate.poll(null) == null && !AutoDiscardState.mutable.value.enabled)
            }
            test("manual input while disabled does not alter paused state") { gate, _ ->
                gate.observe(live())
                val previous = AutoDiscardState.mutable.value
                gate.poll(ack("", "manual"))
                check(previous == AutoDiscardState.mutable.value && gate.poll(null) == null)
            }
            test("model failure waits while armed") { gate, _ ->
                gate.observe(live()); gate.toggle(); gate.observe(live().put("modelFallback",true))
                check(gate.poll(null) == null && AutoDiscardState.mutable.value.enabled)
                gate.observe(live(2)); check(gate.poll(null) != null)
            }
            test("riichi requires a server declaration prompt") { gate, _ ->
                gate.observe(live(kind="riichi")); gate.toggle(); check(gate.poll(null) == null)
            }
            test("timeout keeps opt-in without replaying old decision") { gate, advance ->
                gate.observe(live()); gate.toggle(); advance()
                check(gate.poll(null) == null && AutoDiscardState.mutable.value.enabled)
                gate.observe(live()); check(gate.poll(null) == null)
                gate.observe(live(2)); check(gate.poll(null) != null)
            }
            test("stale rejection does not replay decision") { gate, _ ->
                gate.observe(live()); gate.toggle(); val id=JSONObject(gate.poll(null)!!).getString("id")
                gate.poll(ack(id,"stale")); check(gate.isEnabled() && gate.poll(null) == null)
                gate.observe(live()); check(gate.poll(null) == null)
                gate.observe(live(2)); check(gate.poll(null) != null)
            }
            test("sent input requires server acceptance before next decision") { gate, _ ->
                gate.observe(live()); gate.toggle(); val id=JSONObject(gate.poll(null)!!).getString("id")
                gate.poll(ack(id,"submitted"))
                gate.observe(live().put("canAct",false).put("input",JSONObject("""
                    {"method":".lq.FastTest.inputOperation","payload":{"type":1,"tile":"0m","moqie":false}}
                """)))
                gate.observe(live(2)); check(gate.poll(null) == null)
                gate.poll(ack(id,"accepted")); val next=JSONObject(checkNotNull(gate.poll(null)))
                check(next.getString("id") != id && next.getInt("revision") == 2)
            }
            test("unmatched uplink pauses") { gate, _ ->
                gate.observe(live()); gate.toggle()
                gate.observe(live().put("input",JSONObject("""
                    {"method":".lq.FastTest.inputOperation","payload":{"type":1,"tile":"5m","moqie":false}}
                """)))
                check(gate.poll(null) == null && !AutoDiscardState.mutable.value.enabled)
            }
            fun actionCase(kind: String, type: Int, tile: String = "", consumed: List<String> = emptyList(),
                           combo: List<String> = emptyList(), hand: List<String> = listOf("5mr"), target: Int = 3,
                           response: Boolean = true): JSONObject {
                val state=live(kind=kind).put("hand",JSONArray(hand))
                state.getJSONObject("autoContext").put("phase",if(response) "wait_response" else "wait_act").put("drawnTile","5mr")
                state.put("legalOperations",JSONArray().put(JSONObject().put("type",type).put("combination",JSONArray(combo))))
                state.put("recommendations",JSONArray().put(JSONObject().put("kind",kind).put("tile",tile)
                    .put("target",target).put("consumed",JSONArray(consumed))))
                return state
            }
            fun prepared(state: JSONObject) = checkNotNull(AutoDiscardController.prepareAction(state,state.getJSONArray("recommendations").getJSONObject(0)))
            test("opening dealer accepts client moqie only with complete protocol proof") { gate, _ ->
                val hand=listOf("1m","2m","3m","4m","5m","6m","7m","8m","9m","1p","2p","3p","1s","9s")
                val state=actionCase("discard",1,"9s",hand=hand,response=false)
                state.getJSONArray("recommendations").getJSONObject(0).put("tsumogiri",true)
                state.getJSONObject("autoContext").put("openingDealerDiscard",true).put("dealer",0).put("melds",JSONArray())
                gate.observe(state); gate.toggle()
                val cmd=JSONObject(checkNotNull(gate.poll(null)))
                check(cmd.getJSONObject("action").getBoolean("clientOpeningDraw"))
                val payload=JSONObject("""{"type":1,"tile":"9s","moqie":false}""")
                check(AutoDiscardController.matchesInput(cmd,".lq.FastTest.inputOperation",payload))
                check(!AutoDiscardController.matchesInput(cmd,".lq.FastTest.inputOperation",JSONObject(payload.toString()).put("tile","1s")))
                for ((key,value) in listOf("openingDealerDiscard" to false,"dealer" to 1,"phase" to "wait_response","riichi" to true,"melds" to JSONArray().put(JSONObject()))) {
                    val invalid=JSONObject(cmd.toString())
                    invalid.getJSONObject("autoContext").put(key,value)
                    check(!AutoDiscardController.matchesInput(invalid,".lq.FastTest.inputOperation",payload))
                }
                val shortHand=JSONObject(cmd.toString()).put("hand",JSONArray(hand.dropLast(1)))
                check(!AutoDiscardController.matchesInput(shortHand,".lq.FastTest.inputOperation",payload))
            }
            test("riichi retains red tile and declaration type") { gate, _ ->
                val state=actionCase("riichi",7,"5mr",combo=listOf("5m"),response=false)
                gate.observe(state); gate.toggle()
                val cmd=JSONObject(checkNotNull(gate.poll(null)))
                check(cmd.getJSONObject("action").getInt("type")==7 && cmd.getBoolean("tsumogiri"))
                check(!AutoDiscardController.matchesInput(cmd,".lq.FastTest.inputOperation",JSONObject("""{"type":1,"tile":"0m","moqie":true}""")))
                check(AutoDiscardController.matchesInput(cmd,".lq.FastTest.inputOperation",JSONObject("""{"type":7,"tile":"0m","moqie":true}""")))
            }
            test("chi selects exact red combination index") { _, _ ->
                val state=actionCase("chi",2,"3m",listOf("4m","5mr"),listOf("4m|5m","0m|4m"),listOf("4m","5mr"))
                check(prepared(state).getInt("index")==1)
                state.getJSONArray("recommendations").getJSONObject(0).put("consumed",JSONArray(listOf("4m","5m")))
                check(AutoDiscardController.prepareAction(state,state.getJSONArray("recommendations").getJSONObject(0))==null)
            }
            test("pon and open kan retain target and consumed tiles") { _, _ ->
                for ((kind,type,n) in listOf(Triple("pon",3,2),Triple("kan",5,3))) {
                    val tiles=List(n){"5m"}; val state=actionCase(kind,type,"5m",tiles,listOf(List(n){"5m"}.joinToString("|")),tiles)
                    check(prepared(state).getInt("index")==0 && prepared(state).getInt("target")==3)
                    state.getJSONArray("recommendations").getJSONObject(0).put("target",0)
                    check(AutoDiscardController.prepareAction(state,state.getJSONArray("recommendations").getJSONObject(0))==null)
                }
            }
            test("ankan requires all four exact tiles") { _, _ ->
                val tiles=listOf("5m","5m","5m","5mr")
                check(prepared(actionCase("ankan",4,"5m",tiles,listOf("5m|0m|5m|5m"),tiles,response=false)).getInt("index")==0)
            }
            test("kakan accounts for red tile in the existing pon") { _, _ ->
                val state=actionCase("kakan",6,"5m",listOf("5m","5mr","5m"),listOf("5m|5m|0m|5m"),listOf("5m"),response=false)
                state.getJSONObject("autoContext").put("melds",JSONArray().put(JSONObject().put("kind","pon").put("tiles",JSONArray(listOf("5m","5mr","5m")))))
                check(prepared(state).getInt("index")==0)
            }
            test("ron and tsumo use distinct request methods") { _, _ ->
                check(prepared(actionCase("hora",9,target=3)).getString("method")=="inputChiPengGang")
                check(prepared(actionCase("hora",8,target=0,response=false)).getString("method")=="inputOperation")
            }
            test("pass sends an explicit cancel and never confirms a win") { _, _ ->
                val state=actionCase("pass",2)
                val cmd=JSONObject().put("action",prepared(state))
                check(AutoDiscardController.matchesInput(cmd,".lq.FastTest.inputChiPengGang",JSONObject().put("cancel_operation",true)))
                check(!AutoDiscardController.matchesInput(cmd,".lq.FastTest.inputChiPengGang",JSONObject().put("type",9)))
            }
            test("kita is sanma only and preserves moqie") { _, _ ->
                val state=actionCase("kita",11,"N",hand=listOf("N"),response=false)
                state.put("players",3); state.getJSONObject("autoContext").put("drawnTile","N")
                val cmd=JSONObject().put("action",prepared(state))
                check(AutoDiscardController.matchesInput(cmd,".lq.FastTest.inputOperation",JSONObject().put("type",11).put("moqie",true)))
                state.put("players",4)
                check(AutoDiscardController.prepareAction(state,state.getJSONArray("recommendations").getJSONObject(0))==null)
            }
            test("abort requires server nine terminals prompt") { _, _ ->
                check(prepared(actionCase("abort",10,response=false)).getInt("type")==10)
            }
            test("changed combination index cannot confirm another call") { gate, _ ->
                gate.observe(actionCase("chi",2,"3m",listOf("4m","5mr"),listOf("4m|5m","4m|0m"),listOf("4m","5mr"))); gate.toggle()
                gate.observe(live().put("input",JSONObject("""{"method":".lq.FastTest.inputChiPengGang","payload":{"type":2,"index":0}}""")))
                check(!AutoDiscardState.mutable.value.enabled)
            }
            test("riichi locks ordinary discard to drawn tile") { _, _ ->
                val state=live(); state.getJSONObject("autoContext").put("riichi",true)
                check(AutoDiscardController.prepareAction(state,state.getJSONArray("recommendations").getJSONObject(0))==null)
            }
            test("queued actions wait for their full random delay") { _, _ ->
                for (delay in listOf(2_000L,5_000L)) {
                    var time=0L
                    val gate=AutoDiscardController({}, {time}, {delay})
                    gate.observe(live()); gate.toggle(); check(gate.poll(null)==null)
                    time=delay-1; check(gate.poll(null)==null)
                    time=delay; check(JSONObject(checkNotNull(gate.poll(null))).getLong("delayMs")==delay)
                }
            }
            test("heartbeat renews sequence without restarting delay") { _, _ ->
                var time=0L
                val gate=AutoDiscardController({}, {time}, {2_000L})
                gate.observe(live()); gate.toggle()
                time=1_500; gate.observe(live().put("sourceSequence",9)); check(gate.poll(null)==null)
                time=2_000; check(JSONObject(checkNotNull(gate.poll(null))).getLong("sourceSequence")==9L)
            }
            test("changed decision gets its own full delay") { _, _ ->
                var time=0L
                val gate=AutoDiscardController({}, {time}, {2_000L})
                gate.observe(live()); gate.toggle(); gate.observe(live(2))
                time=1_999; check(gate.poll(null)==null && AutoDiscardState.mutable.value.enabled)
                time=2_000; check(JSONObject(checkNotNull(gate.poll(null))).getInt("revision")==2)
            }
            test("pause during delay never publishes input") { _, _ ->
                var time=0L
                val gate=AutoDiscardController({}, {time}, {5_000L})
                gate.observe(live()); gate.toggle(); gate.pause("manual")
                time=5_000; check(gate.poll(null)==null)
            }
            test("production random delay always stays within two to five seconds") { _, _ ->
                repeat(30) {
                    var time=0L
                    val gate=AutoDiscardController({}, {time})
                    gate.observe(live()); gate.toggle(); check(gate.poll(null)==null)
                    time=5_000
                    check(JSONObject(checkNotNull(gate.poll(null))).getLong("delayMs") in 2_000L..5_000L)
                }
            }
            test("custom delay boundaries and fixed delay") { _, _ ->
                for (value in listOf(0, 1, 13, 50)) {
                    val range=AutoDelayRange(value,value)
                    repeat(10) { check(range.randomDelayMs()==value*100L) }
                    var time=0L
                    val gate=AutoDiscardController({}, {time}, range::randomDelayMs)
                    gate.observe(live()); gate.toggle()
                    if (value>0) { time=value*100L-1; check(gate.poll(null)==null) }
                    time=value*100L
                    check(JSONObject(checkNotNull(gate.poll(null))).getLong("delayMs")==time)
                }
                repeat(100) { check(AutoDelayRange(10,30).randomDelayMs() in 1_000L..3_000L) }
                check(AutoDelayRange(20,30).withMinimum(40)==AutoDelayRange(40,40))
                check(AutoDelayRange(20,30).withMaximum(10)==AutoDelayRange(10,10))
            }
            test("delay edits affect new decisions without restarting a queued wait") { _, _ ->
                var time=0L
                var range=AutoDelayRange(10,10)
                val gate=AutoDiscardController({}, {time}, {range.randomDelayMs()})
                gate.observe(live()); gate.toggle()
                range=AutoDelayRange(30,30)
                time=999; check(gate.poll(null)==null)
                time=1_000; check(JSONObject(checkNotNull(gate.poll(null))).getLong("delayMs")==1_000L)
                gate.observe(live(2))
                time=3_999; check(gate.poll(null)==null)
                time=4_000; check(JSONObject(checkNotNull(gate.poll(null))).getLong("delayMs")==3_000L)
            }
            test("delay settings persist and recover invalid saved values") { _, _ ->
                val isolated=object : android.content.ContextWrapper(targetContext) {
                    override fun getSharedPreferences(name: String, mode: Int) = super.getSharedPreferences("delay-checks-$name",mode)
                }
                val prefs=isolated.getSharedPreferences("unattended",0)
                try {
                    prefs.edit().clear().commit()
                    check(AutoDelaySettings.read(isolated)==AutoDelayRange(20,50))
                    AutoDelaySettings.save(isolated,AutoDelayRange(13,27))
                    check(AutoDelaySettings.read(isolated)==AutoDelayRange(13,27))
                    prefs.edit().putInt("delay_min_tenths",99).putInt("delay_max_tenths",-1).commit()
                    check(AutoDelaySettings.read(isolated)==AutoDelayRange(0,50))
                    prefs.edit().putString("delay_min_tenths","bad").remove("delay_max_tenths").commit()
                    check(AutoDelaySettings.read(isolated)==AutoDelayRange(20,50))
                } finally { prefs.edit().clear().commit() }
            }
            test("round settlement resumes automatically with a new round") { gate, _ ->
                gate.observe(live()); gate.toggle()
                gate.observe(JSONObject().put("status","waiting"))
                check(gate.isEnabled() && gate.poll(null)==null)
                gate.observe(live(2)); check(gate.poll(null)!=null)
            }
            test("external timeout does not disable unattended mode") { gate, _ ->
                gate.observe(live()); gate.toggle(); gate.poll(ack("","external"))
                check(gate.isEnabled() && gate.poll(null)==null)
                gate.observe(live(2)); check(gate.poll(null)!=null)
            }
            test("restored opt-in waits for sync and persists explicit closure") { _, _ ->
                val saved=mutableListOf<Boolean>()
                val gate=AutoDiscardController({}, {0L}, {0L}, initialEnabled=true, modeChanged={saved.add(it)})
                check(gate.isEnabled() && gate.needsSync() && gate.poll(null)==null)
                gate.observe(live()); check(gate.poll(null)!=null)
                gate.pause("explicit stop"); gate.observe(live(2))
                check(!gate.isEnabled() && gate.poll(null)==null && saved.last()==false)
            }
            test("game relaunch waits ten seconds and backs off") { _, _ ->
                val recovery=UnattendedRecovery()
                check(!recovery.shouldLaunch(true,false,0))
                check(!recovery.shouldLaunch(true,false,9_999))
                check(recovery.shouldLaunch(true,false,10_000))
                check(!recovery.shouldLaunch(true,false,39_999))
                check(recovery.shouldLaunch(true,false,40_000))
                check(!recovery.shouldLaunch(true,false,99_999))
                check(recovery.shouldLaunch(true,false,100_000))
                check(!recovery.shouldLaunch(true,true,101_000))
                check(!recovery.shouldLaunch(true,false,102_000))
                check(!recovery.shouldLaunch(false,false,120_000))
            }
            test("temporary game proof wait keeps the same pending command") { gate, _ ->
                gate.observe(live()); gate.toggle()
                val cmd=JSONObject(checkNotNull(gate.poll(null)))
                gate.poll(ack(cmd.getString("id"),"waiting"))
                check(gate.isEnabled() && JSONObject(checkNotNull(gate.poll(null))).getString("id")==cmd.getString("id"))
            }
            test("validated custom model can arm and continue unattended") { gate, _ ->
                gate.observe(live().put("modelExperimental",true)); gate.toggle()
                val command=JSONObject(checkNotNull(gate.poll(null)))
                check(gate.isEnabled() && command.getJSONObject("action").getString("kind")=="discard")
                gate.poll(ack(command.getString("id"),"submitted"))
                gate.observe(live().put("modelExperimental",true).put("canAct",false).put("input",JSONObject("""
                    {"method":".lq.FastTest.inputOperation","payload":{"type":1,"tile":"0m","moqie":false}}
                """)))
                gate.observe(live(2).put("modelExperimental",true))
                check(gate.poll(null)==null)
                gate.poll(ack(command.getString("id"),"accepted"))
                check(gate.isEnabled() && JSONObject(checkNotNull(gate.poll(null))).getInt("revision")==2)
            }
            test("custom model fallback or missing proof still blocks unattended input") { gate, _ ->
                gate.observe(live().put("modelExperimental",true).put("modelFallback",true)); gate.toggle()
                check(gate.isEnabled() && gate.poll(null)==null)
                gate.observe(live(2).put("modelExperimental",true)); check(gate.poll(null)!=null)
                gate.observe(live(3).put("modelExperimental",true).put("modelFallback",true))
                check(gate.isEnabled() && gate.poll(null)==null)
                gate.observe(live(4).put("modelExperimental",true).put("sourceSequence",0))
                check(gate.isEnabled() && gate.poll(null)==null)
            }
            test("custom model preserves stale rejection and manual stop") { gate, _ ->
                gate.observe(live().put("modelExperimental",true)); gate.toggle()
                val command=JSONObject(checkNotNull(gate.poll(null)))
                gate.poll(ack(command.getString("id"),"stale"))
                gate.observe(live().put("modelExperimental",true)); check(gate.poll(null)==null)
                gate.observe(live(2).put("modelExperimental",true)); check(gate.poll(null)!=null)
                gate.poll(ack("","manual"))
                check(!gate.isEnabled() && gate.poll(null)==null)
            }
            DiagnosticsClearChecks.run(targetContext)
            sendStatus(0, Bundle().apply { putString("stream", "PASS diagnostic cleanup scopes, rotations and legacy migration\n") })
            ApkUpdateChecks.run(targetContext)
            sendStatus(0, Bundle().apply { putString("stream", "PASS APK update identity, integrity, routing and cancellation checks\n") })
            CatalogChecks.run(targetContext, context)
            sendStatus(0, Bundle().apply { putString("stream", "PASS unlock catalog update, recovery and GitHub routes\n") })
            if (!controllerOnly) {
                ModelContractChecks.run(targetContext, context)
                sendStatus(0, Bundle().apply { putString("stream", "PASS ONNX/JNI contracts: four-player and sanma, bool/float masks and crossed-pair rejection\n") })
            }
            output.putString("stream", "$tests controller checks passed\n")
            finish(-1, output)
        }.onFailure {
            output.putString("stream", it.stackTraceToString()); finish(1, output)
        }
    }
}
