#include <jni.h>
#include <cstdint>
#include <cstddef>

extern "C" {
void majmax_ai_reset();
char *majmax_ai_frame(uint64_t connection, uint8_t direction, const uint8_t *data, size_t len);
char *majmax_ai_self_test();
void majmax_ai_free(char *value);
using PolicyCallback = int32_t (*)(uint8_t, const float *, size_t, float *, size_t);
using PolicyActivate = int32_t (*)(uint8_t);
void majmax_ai_register_policy_callbacks(PolicyCallback callback, PolicyActivate activate);
void majmax_ai_set_policy_mask(uint8_t mask);
}

static JavaVM *gVm = nullptr;
static jclass gPolicyClass = nullptr;
static jmethodID gInferMethod = nullptr;
static jmethodID gActivateMethod = nullptr;

static int32_t activateOnnx(uint8_t players) {
    if (!gVm || !gPolicyClass || !gActivateMethod) return -1;
    JNIEnv *env = nullptr;
    bool attached = false;
    jint state = gVm->GetEnv(reinterpret_cast<void **>(&env), JNI_VERSION_1_6);
    if (state == JNI_EDETACHED) {
        if (gVm->AttachCurrentThread(&env, nullptr) != JNI_OK) return -1;
        attached = true;
    } else if (state != JNI_OK) {
        return -1;
    }
    jint status = env->CallStaticIntMethod(gPolicyClass, gActivateMethod, static_cast<jint>(players));
    if (env->ExceptionCheck()) { env->ExceptionClear(); status = -1; }
    if (attached) gVm->DetachCurrentThread();
    return status;
}

static int32_t inferOnnx(uint8_t players, const float *obs, size_t obsSize,
                         float *logits, size_t logitsSize) {
    if (!gVm || !gPolicyClass || !gInferMethod || obsSize > 2048 || logitsSize > 128) return -1;
    JNIEnv *env = nullptr;
    bool attached = false;
    jint state = gVm->GetEnv(reinterpret_cast<void **>(&env), JNI_VERSION_1_6);
    if (state == JNI_EDETACHED) {
        if (gVm->AttachCurrentThread(&env, nullptr) != JNI_OK) return -1;
        attached = true;
    } else if (state != JNI_OK) {
        return -1;
    }
    jint status = -1;
    jfloatArray observation = env->NewFloatArray(static_cast<jsize>(obsSize));
    if (observation) {
        env->SetFloatArrayRegion(observation, 0, static_cast<jsize>(obsSize), obs);
        jobject result = env->CallStaticObjectMethod(gPolicyClass, gInferMethod,
                static_cast<jint>(players), observation);
        if (!env->ExceptionCheck() && result) {
            auto values = static_cast<jfloatArray>(result);
            if (static_cast<size_t>(env->GetArrayLength(values)) == logitsSize) {
                env->GetFloatArrayRegion(values, 0, static_cast<jsize>(logitsSize), logits);
                if (!env->ExceptionCheck()) status = 0;
            }
            env->DeleteLocalRef(result);
        }
        if (env->ExceptionCheck()) env->ExceptionClear();
        env->DeleteLocalRef(observation);
    }
    if (env->ExceptionCheck()) env->ExceptionClear();
    if (attached) gVm->DetachCurrentThread();
    return status;
}

extern "C" JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM *vm, void *) {
    gVm = vm;
    JNIEnv *env = nullptr;
    if (vm->GetEnv(reinterpret_cast<void **>(&env), JNI_VERSION_1_6) != JNI_OK) return JNI_ERR;
    jclass local = env->FindClass("com/yefeng/majmax/hookprobe/manager/OnnxModelStore");
    if (!local || env->ExceptionCheck()) {
        env->ExceptionClear();
        return JNI_VERSION_1_6;
    }
    gPolicyClass = static_cast<jclass>(env->NewGlobalRef(local));
    env->DeleteLocalRef(local);
    gInferMethod = env->GetStaticMethodID(gPolicyClass, "infer", "(I[F)[F");
    gActivateMethod = env->GetStaticMethodID(gPolicyClass, "activate", "(I)I");
    if (!gInferMethod || !gActivateMethod || env->ExceptionCheck()) {
        env->ExceptionClear();
        gPolicyClass = nullptr;
        return JNI_VERSION_1_6;
    }
    majmax_ai_register_policy_callbacks(&inferOnnx, &activateOnnx);
    return JNI_VERSION_1_6;
}

static jstring takeString(JNIEnv *env, char *value) {
    if (!value) return nullptr;
    jstring result = env->NewStringUTF(value);
    majmax_ai_free(value);
    return result;
}

extern "C" JNIEXPORT void JNICALL
Java_com_yefeng_majmax_hookprobe_manager_AiNative_reset(JNIEnv *, jclass) {
    majmax_ai_reset();
}

extern "C" JNIEXPORT void JNICALL
Java_com_yefeng_majmax_hookprobe_manager_AiNative_configurePolicy(JNIEnv *, jclass, jint mask) {
    majmax_ai_set_policy_mask(static_cast<uint8_t>(mask));
}

extern "C" JNIEXPORT jstring JNICALL
Java_com_yefeng_majmax_hookprobe_manager_AiNative_frame(
        JNIEnv *env, jclass, jlong connection, jint direction, jbyteArray bytes) {
    if (!bytes) return nullptr;
    jsize size = env->GetArrayLength(bytes);
    if (size > 1024 * 1024) return nullptr;
    jbyte *data = env->GetByteArrayElements(bytes, nullptr);
    if (!data) return nullptr;
    char *result = majmax_ai_frame(static_cast<uint64_t>(connection),
            static_cast<uint8_t>(direction), reinterpret_cast<uint8_t *>(data), size);
    env->ReleaseByteArrayElements(bytes, data, JNI_ABORT);
    return takeString(env, result);
}

extern "C" JNIEXPORT jstring JNICALL
Java_com_yefeng_majmax_hookprobe_manager_AiNative_selfTest(JNIEnv *env, jclass) {
    return takeString(env, majmax_ai_self_test());
}
