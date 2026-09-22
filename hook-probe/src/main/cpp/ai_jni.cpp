#include <jni.h>
#include <cstdint>
#include <cstddef>

extern "C" {
void majmax_ai_reset();
char *majmax_ai_frame(uint64_t connection, uint8_t direction, const uint8_t *data, size_t len);
char *majmax_ai_self_test();
void majmax_ai_free(char *value);
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
