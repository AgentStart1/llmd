package com.storytellerf.llmd.ipc;

import com.storytellerf.llmd.ipc.ILlmdChatCallback;

interface ILlmdService {
    void healthAsync(ILlmdChatCallback callback);
    void listModelsAsync(ILlmdChatCallback callback);
    void chatCompletionAsync(String requestJson, ILlmdChatCallback callback);
}
