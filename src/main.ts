import { createApp } from "vue";
import ArcoVue from "@arco-design/web-vue";
import ArcoVueIcon from "@arco-design/web-vue/es/icon";
import "@arco-design/web-vue/dist/arco.css";
import App from "./App.vue";
import i18n from "./i18n";
import { reportUiError } from "./utils/run_log_report";
import "./styles/global.scss";

const app = createApp(App);
app.use(ArcoVue);
app.use(ArcoVueIcon);
app.use(i18n);

// 没人接住的错误也要进运行日志：这类问题在控制台里一闪而过，关掉窗口就没了，
// 用户报「界面崩了」时如果日志里没有，也只能让人再重现一次。
window.onerror = (message, _source, _lineno, _colno, error) => {
  reportUiError("未捕获的错误：", error ?? message);
};
window.onunhandledrejection = (event) => {
  reportUiError("未处理的 Promise 拒绝：", event.reason);
};
// 自己接住 Vue 的组件错误后，Vue 不再向上抛同一个错误，因此不会重复记录；
// 控制台那一份由上报通道自己写，信息也不会丢。
app.config.errorHandler = (error, _instance, info) => {
  reportUiError(`组件渲染出错（${info}）：`, error);
};

app.mount("#app");
